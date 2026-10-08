//! Web Push to paired phones (PRD D-09, D-19, D-21, D-23).
//!
//! The daemon signs with its own VAPID key (RFC 8292, ES256) and encrypts
//! each message for the phone's subscription (RFC 8291, aes128gcm), with
//! `ring` for the curve, HMAC and AES-GCM and `ureq` over rustls for the
//! POST: both are already in the build, and no other TLS stack is added.
//!
//! What is sent: one notification per root or escalated child entering Needs You
//! or Done, as data: the task as
//! the title, the state (`needs_you` or `done`) and the project; never
//! terminal content and never a sentence. The words for the state belong to
//! the phone's language, so the phone page hands them to its service worker
//! and the worker composes the body (`web/public/m/sw.js`). A later
//! transition of the same agent replaces it (same tag). Agents the desktop
//! has since made Seen ride along as tags to close.

use std::collections::BTreeSet;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};

use super::projection::AgentKey;
use super::store::{PushMode, PushSubscription};
#[cfg(test)]
use herdr_core::agent_state::push::VANISHED_TTL;
pub use herdr_core::agent_state::push::{Notice, NoticeState, Transitions};

const RECORD_SIZE: u32 = 4096;
const TTL_SECS: u64 = 60 * 60;
const JWT_LIFETIME_SECS: u64 = 12 * 60 * 60;
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// The push services a subscription may point at. A paired phone chooses
/// the endpoint, so the daemon only ever posts to a known push service.
const PUSH_HOSTS: [&str; 4] = [
    "push.apple.com",
    "fcm.googleapis.com",
    "push.services.mozilla.com",
    "notify.windows.com",
];

fn host_allowed(host: &str) -> bool {
    PUSH_HOSTS
        .iter()
        .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
}

/// Whether the daemon may post to this endpoint: https to a known push
/// service; a debug build also accepts `http://127.0.0.1:<port>/...` for the
/// e2e fake push service.
pub fn endpoint_allowed(endpoint: &str) -> bool {
    if endpoint.len() > 2048 || endpoint.bytes().any(|b| b.is_ascii_control()) {
        return false;
    }
    // Parsed as the HTTP client will read it; any userinfo is refused
    // outright, because a client splits `host:port@other` differently from a
    // naive check and would connect to `other`.
    let Ok(uri) = endpoint.parse::<axum::http::Uri>() else {
        return false;
    };
    let Some(authority) = uri.authority() else {
        return false;
    };
    if authority.as_str().contains('@') {
        return false;
    }
    match uri.scheme_str() {
        Some("https") => {
            matches!(authority.port_u16(), None | Some(443)) && host_allowed(authority.host())
        }
        Some("http") if cfg!(debug_assertions) => {
            authority.host() == "127.0.0.1" && authority.port_u16().is_some()
        }
        _ => false,
    }
}

/// A subscription the phone sent, checked for shape before it is stored.
pub fn subscription_valid(push: &PushSubscription) -> bool {
    let key = URL_SAFE_NO_PAD.decode(push.p256dh.trim_end_matches('='));
    let auth = URL_SAFE_NO_PAD.decode(push.auth.trim_end_matches('='));
    endpoint_allowed(&push.endpoint)
        && key.is_ok_and(|key| key.len() == 65 && key[0] == 4)
        && auth.is_ok_and(|auth| auth.len() == 16)
}

/// The daemon's VAPID key pair.
pub struct Vapid {
    key: EcdsaKeyPair,
}

impl Vapid {
    /// A new key pair and its PKCS#8 bytes to store.
    pub fn generate() -> Result<(Self, Vec<u8>), String> {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
            .map_err(|_| "VAPID key generation failed".to_owned())?;
        let bytes = pkcs8.as_ref().to_vec();
        Ok((Self::from_pkcs8(&bytes)?, bytes))
    }

    pub fn from_pkcs8(bytes: &[u8]) -> Result<Self, String> {
        EcdsaKeyPair::from_pkcs8(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            bytes,
            &SystemRandom::new(),
        )
        .map(|key| Self { key })
        .map_err(|_| "the stored VAPID key is unreadable".to_owned())
    }

    /// The uncompressed public key, base64url: the phone's `applicationServerKey`.
    pub fn public_key(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.key.public_key().as_ref())
    }

    /// The `Authorization` header value for one push service origin.
    pub fn authorization(
        &self,
        audience: &str,
        subject: &str,
        now_secs: u64,
    ) -> Result<String, String> {
        let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = URL_SAFE_NO_PAD.encode(
            json!({"aud": audience, "exp": now_secs + JWT_LIFETIME_SECS, "sub": subject})
                .to_string(),
        );
        let input = format!("{header}.{claims}");
        let signature = self
            .key
            .sign(&SystemRandom::new(), input.as_bytes())
            .map_err(|_| "VAPID signing failed".to_owned())?;
        Ok(format!(
            "vapid t={input}.{}, k={}",
            URL_SAFE_NO_PAD.encode(signature.as_ref()),
            self.public_key()
        ))
    }
}

fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    let mut context = ring::hmac::Context::with_key(&key);
    for part in parts {
        context.update(part);
    }
    let mut out = [0_u8; 32];
    out.copy_from_slice(context.sign().as_ref());
    out
}

/// RFC 8291 section 3.4 once the ECDH secret is known: derives the key and
/// nonce and returns the whole aes128gcm body (header, then one record).
pub fn encrypt_with_secret(
    ecdh_secret: &[u8],
    auth_secret: &[u8],
    ua_public: &[u8],
    as_public: &[u8],
    salt: &[u8; 16],
    plaintext: &[u8],
) -> Result<Vec<u8>, String> {
    let prk_key = hmac(auth_secret, &[ecdh_secret]);
    let ikm = hmac(&prk_key, &[b"WebPush: info\0", ua_public, as_public, &[1]]);
    let prk = hmac(salt, &[&ikm]);
    let cek = hmac(&prk, &[b"Content-Encoding: aes128gcm\0", &[1]]);
    let nonce = hmac(&prk, &[b"Content-Encoding: nonce\0", &[1]]);
    let key = ring::aead::UnboundKey::new(&ring::aead::AES_128_GCM, &cek[..16])
        .map_err(|_| "content key rejected".to_owned())?;
    let key = ring::aead::LessSafeKey::new(key);
    let nonce = ring::aead::Nonce::try_assume_unique_for_key(&nonce[..12])
        .map_err(|_| "nonce rejected".to_owned())?;
    let mut record = plaintext.to_vec();
    // The last (and only) record's padding delimiter.
    record.push(2);
    if record.len() + 16 > RECORD_SIZE as usize {
        return Err("the message is longer than one record".to_owned());
    }
    key.seal_in_place_append_tag(nonce, ring::aead::Aad::empty(), &mut record)
        .map_err(|_| "encryption failed".to_owned())?;
    let mut body = Vec::with_capacity(16 + 4 + 1 + as_public.len() + record.len());
    body.extend_from_slice(salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(u8::try_from(as_public.len()).map_err(|_| "key id too long".to_owned())?);
    body.extend_from_slice(as_public);
    body.extend_from_slice(&record);
    Ok(body)
}

/// Encrypts one message for a subscription with a fresh sender key and salt.
pub fn encrypt(push: &PushSubscription, plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let ua_public = URL_SAFE_NO_PAD
        .decode(push.p256dh.trim_end_matches('='))
        .map_err(|_| "the subscription key is not base64url".to_owned())?;
    let auth = URL_SAFE_NO_PAD
        .decode(push.auth.trim_end_matches('='))
        .map_err(|_| "the subscription secret is not base64url".to_owned())?;
    let rng = SystemRandom::new();
    let private = ring::agreement::EphemeralPrivateKey::generate(&ring::agreement::ECDH_P256, &rng)
        .map_err(|_| "sender key generation failed".to_owned())?;
    let as_public = private
        .compute_public_key()
        .map_err(|_| "sender key generation failed".to_owned())?;
    let mut salt = [0_u8; 16];
    getrandom::getrandom(&mut salt).map_err(|error| error.to_string())?;
    let peer =
        ring::agreement::UnparsedPublicKey::new(&ring::agreement::ECDH_P256, ua_public.clone());
    let secret = ring::agreement::agree_ephemeral(private, &peer, |secret| secret.to_vec())
        .map_err(|_| "the subscription key is not a P-256 point".to_owned())?;
    encrypt_with_secret(
        &secret,
        &auth,
        &ua_public,
        as_public.as_ref(),
        &salt,
        plaintext,
    )
}

/// `https://host[:port]` of an endpoint: the JWT audience.
fn origin_of(endpoint: &str) -> String {
    let (scheme, rest) = endpoint.split_once("://").unwrap_or(("https", endpoint));
    let host = rest.split('/').next().unwrap_or("");
    format!("{scheme}://{host}")
}

/// How a send ended.
#[derive(Debug, Eq, PartialEq)]
pub enum SendOutcome {
    Delivered,
    /// The push service says the subscription no longer exists (404, 410).
    Gone(u16),
    Failed(String),
}

pub fn send(
    vapid: &Vapid,
    subject: &str,
    push: &PushSubscription,
    payload: &Value,
    now_secs: u64,
) -> SendOutcome {
    if !endpoint_allowed(&push.endpoint) {
        return SendOutcome::Failed("the endpoint is not a known push service".to_owned());
    }
    let body = match encrypt(push, payload.to_string().as_bytes()) {
        Ok(body) => body,
        Err(message) => return SendOutcome::Failed(message),
    };
    let authorization = match vapid.authorization(&origin_of(&push.endpoint), subject, now_secs) {
        Ok(value) => value,
        Err(message) => return SendOutcome::Failed(message),
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .http_status_as_error(false)
        // The allowlisted service answers itself; a redirect could send the
        // signed request anywhere.
        .max_redirects(0)
        .build()
        .into();
    let topic = URL_SAFE_NO_PAD.encode(
        &ring::digest::digest(
            &ring::digest::SHA256,
            payload
                .get("tag")
                .and_then(Value::as_str)
                .unwrap_or("")
                .as_bytes(),
        )
        .as_ref()[..24],
    );
    let result = agent
        .post(&push.endpoint)
        .header("Authorization", &authorization)
        .header("Content-Encoding", "aes128gcm")
        .header("Content-Type", "application/octet-stream")
        .header("TTL", &TTL_SECS.to_string())
        .header("Urgency", "high")
        .header("Topic", &topic)
        .send(&body[..]);
    match result {
        Ok(response) => match response.status().as_u16() {
            200..=299 => SendOutcome::Delivered,
            status @ (404 | 410) => SendOutcome::Gone(status),
            status => SendOutcome::Failed(format!("the push service answered {status}")),
        },
        Err(error) => SendOutcome::Failed(error.to_string()),
    }
}

/// Whether a notification may go out under the operator's mode: Off never,
/// app-closed-only while no desktop or web shell is connected, Always always.
pub fn mode_allows(mode: PushMode, renderers: usize) -> bool {
    match mode {
        PushMode::Off => false,
        PushMode::AppClosed => renderers == 0,
        PushMode::Always => true,
    }
}

/// The JSON a phone's service worker receives.
pub fn payload(notice: &Notice, clear: &BTreeSet<AgentKey>) -> Value {
    json!({
        "title": notice.title,
        "state": notice.state.as_str(),
        "place": notice.place,
        "tag": notice.key.tag(),
        "device_id": notice.key.device_id,
        "pane_id": notice.key.pane_id,
        "clear": clear.iter().map(AgentKey::tag).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobile::projection::project;

    fn b64(text: &str) -> Vec<u8> {
        URL_SAFE_NO_PAD.decode(text).unwrap()
    }

    /// RFC 8291 Appendix A: the example's inputs produce its header and
    /// ciphertext byte for byte.
    #[test]
    fn rfc8291_appendix_a_vector() {
        let as_public = b64(
            "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8",
        );
        let ua_public = b64(
            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
        );
        let secret = b64("kyrL1jIIOHEzg3sM2ZWRHDRB62YACZhhSlknJ672kSs");
        let auth = b64("BTBZMqHH6r4Tts7J_aSIgg");
        let salt: [u8; 16] = b64("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let plaintext = b64("V2hlbiBJIGdyb3cgdXAsIEkgd2FudCB0byBiZSBhIHdhdGVybWVsb24");
        let body =
            encrypt_with_secret(&secret, &auth, &ua_public, &as_public, &salt, &plaintext).unwrap();
        let expected_header = b64(
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8",
        );
        let expected_cipher =
            b64("8pfeW0KbunFT06SuDKoJH9Ql87S1QUrdirN6GcG7sFz1y1sqLgVi1VhjVkHsUoEsbI_0LpXMuGvnzQ");
        assert_eq!(&body[..86], &expected_header[..]);
        assert_eq!(&body[86..], &expected_cipher[..]);
        // The intermediate keys the appendix lists.
        let prk_key = hmac(&auth, &[&secret]);
        assert_eq!(
            URL_SAFE_NO_PAD.encode(prk_key),
            "Snr3JMxaHVDXHWJn5wdC52WjpCtd2EIEGBykDcZW32k"
        );
        let ikm = hmac(
            &prk_key,
            &[b"WebPush: info\0", &ua_public, &as_public, &[1]],
        );
        assert_eq!(
            URL_SAFE_NO_PAD.encode(ikm),
            "S4lYMb_L0FxCeq0WhDx813KgSYqU26kOyzWUdsXYyrg"
        );
    }

    /// A message encrypted for a subscription decrypts with the phone's key.
    #[test]
    fn a_message_round_trips_through_ecdh() {
        let rng = SystemRandom::new();
        let phone =
            ring::agreement::EphemeralPrivateKey::generate(&ring::agreement::ECDH_P256, &rng)
                .unwrap();
        let phone_public = phone.compute_public_key().unwrap();
        let auth = [7_u8; 16];
        let push = PushSubscription {
            endpoint: "https://web.push.apple.com/abc".into(),
            p256dh: URL_SAFE_NO_PAD.encode(phone_public.as_ref()),
            auth: URL_SAFE_NO_PAD.encode(auth),
        };
        assert!(subscription_valid(&push));
        let body = encrypt(&push, b"{\"title\":\"t\"}").unwrap();
        let salt: [u8; 16] = body[..16].try_into().unwrap();
        let as_public = body[21..86].to_vec();
        let peer =
            ring::agreement::UnparsedPublicKey::new(&ring::agreement::ECDH_P256, as_public.clone());
        let secret =
            ring::agreement::agree_ephemeral(phone, &peer, |secret| secret.to_vec()).unwrap();
        let prk_key = hmac(&auth, &[&secret]);
        let ikm = hmac(
            &prk_key,
            &[b"WebPush: info\0", phone_public.as_ref(), &as_public, &[1]],
        );
        let prk = hmac(&salt, &[&ikm]);
        let cek = hmac(&prk, &[b"Content-Encoding: aes128gcm\0", &[1]]);
        let nonce = hmac(&prk, &[b"Content-Encoding: nonce\0", &[1]]);
        let key = ring::aead::LessSafeKey::new(
            ring::aead::UnboundKey::new(&ring::aead::AES_128_GCM, &cek[..16]).unwrap(),
        );
        let mut record = body[86..].to_vec();
        let plain = key
            .open_in_place(
                ring::aead::Nonce::try_assume_unique_for_key(&nonce[..12]).unwrap(),
                ring::aead::Aad::empty(),
                &mut record,
            )
            .unwrap();
        assert_eq!(plain, b"{\"title\":\"t\"}\x02");
    }

    #[test]
    fn the_vapid_token_verifies_with_the_public_key() {
        let (vapid, pkcs8) = Vapid::generate().unwrap();
        let reloaded = Vapid::from_pkcs8(&pkcs8).unwrap();
        assert_eq!(vapid.public_key(), reloaded.public_key());
        let header = reloaded
            .authorization("https://web.push.apple.com", "mailto:hide@localhost", 1_000)
            .unwrap();
        let token = header
            .strip_prefix("vapid t=")
            .unwrap()
            .split(", k=")
            .next()
            .unwrap();
        let (input, signature) = token.rsplit_once('.').unwrap();
        let claims: Value = serde_json::from_slice(&b64(input.split('.').nth(1).unwrap())).unwrap();
        assert_eq!(claims["aud"], "https://web.push.apple.com");
        assert_eq!(claims["exp"], 1_000 + JWT_LIFETIME_SECS);
        let public = b64(&reloaded.public_key());
        ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, public)
            .verify(input.as_bytes(), &b64(signature))
            .expect("signature verifies");
    }

    #[test]
    fn only_known_push_services_are_reachable() {
        assert!(endpoint_allowed("https://web.push.apple.com/QGuW"));
        assert!(endpoint_allowed("https://fcm.googleapis.com/fcm/send/x"));
        assert!(endpoint_allowed(
            "https://updates.push.services.mozilla.com/wpush/v2/x"
        ));
        assert!(!endpoint_allowed("https://example.com/push"));
        assert!(!endpoint_allowed("https://push.apple.com.evil.test/x"));
        assert!(!endpoint_allowed("https://user@web.push.apple.com/x"));
        assert!(!endpoint_allowed(
            "https://web.push.apple.com:1@evil.test/x"
        ));
        assert!(!endpoint_allowed("https://fcm.googleapis.com:8443/x"));
        assert!(!endpoint_allowed("http://web.push.apple.com/x"));
        assert!(!endpoint_allowed("http://10.0.0.1:80/x"));
        // The e2e fake service, in this debug test build only.
        assert_eq!(
            endpoint_allowed("http://127.0.0.1:4000/push/1"),
            cfg!(debug_assertions)
        );
    }

    fn rest(agents: Value) -> Value {
        json!({"navigator": {"agents": agents, "workspaces": [
            {"label": "herdr-ide", "is_git": true, "checkouts": [{"label": "main", "branch": "main", "tabs": [{"panes": [{"id": "w1:p1"}, {"id": "w1:p2"}]}]}]}
        ]}})
    }

    fn row(pane: &str, group: &str, demand: &str, parent: Option<&str>) -> Value {
        json!({"pane_id": pane, "group": group, "demand": demand, "identity_label": format!("task {pane}"),
               "agent_kind": "claude", "lineage_parent_pane_id": parent})
    }

    #[test]
    fn roots_announce_needs_you_and_done_once_each() {
        let mut transitions = Transitions::default();
        let seed = project(
            &rest(json!([row("w1:p1", "needs_you", "question", None)])),
            "local",
        );
        assert_eq!(
            transitions.observe(&seed),
            (Vec::new(), BTreeSet::new()),
            "the first look only seeds"
        );
        let working = project(
            &rest(json!([row("w1:p1", "working", "none", None)])),
            "local",
        );
        assert!(transitions.observe(&working).0.is_empty());
        let asking = project(
            &rest(json!([row("w1:p1", "needs_you", "approval", None)])),
            "local",
        );
        let (notices, _) = transitions.observe(&asking);
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].title, "task w1:p1");
        assert_eq!(notices[0].state, NoticeState::NeedsYou);
        assert_eq!(notices[0].place, "herdr-ide");
        assert!(
            transitions.observe(&asking).0.is_empty(),
            "no repeat without a transition"
        );
        let done = project(&rest(json!([row("w1:p1", "done", "none", None)])), "local");
        let finished = transitions.observe(&done).0;
        assert_eq!(finished[0].state, NoticeState::Done);
        assert_eq!(finished[0].place, "herdr-ide");
        let seen = project(&rest(json!([row("w1:p1", "seen", "none", None)])), "local");
        let (notices, cleared) = transitions.observe(&seen);
        assert!(notices.is_empty());
        assert_eq!(
            cleared
                .into_iter()
                .map(|key| key.pane_id)
                .collect::<Vec<_>>(),
            ["w1:p1"]
        );
    }

    /// The wire shape the service worker reads: the state key and the place,
    /// no sentence in any language.
    #[test]
    fn the_payload_carries_state_and_place_and_no_sentence() {
        let notice = Notice {
            key: AgentKey {
                device_id: "mini".into(),
                pane_id: "w2:p1".into(),
            },
            title: "fix the build".into(),
            state: NoticeState::Done,
            place: "contong".into(),
        };
        let clear = BTreeSet::from([AgentKey {
            device_id: "local".into(),
            pane_id: "w1:p1".into(),
        }]);
        assert_eq!(
            payload(&notice, &clear),
            json!({
                "title": "fix the build",
                "state": "done",
                "place": "contong",
                "tag": "mini|w2:p1",
                "device_id": "mini",
                "pane_id": "w2:p1",
                "clear": ["local|w1:p1"],
            })
        );
        let asking = Notice {
            state: NoticeState::NeedsYou,
            place: String::new(),
            ..notice
        };
        let payload = payload(&asking, &BTreeSet::new());
        assert_eq!(payload["state"], "needs_you");
        assert_eq!(payload["place"], "");
        assert!(payload.get("body").is_none());
    }

    #[test]
    fn an_ordinary_descendant_question_never_announces_its_root() {
        let mut transitions = Transitions::default();
        let quiet = project(
            &rest(json!([
                row("w1:p1", "working", "none", None),
                row("w1:p2", "working", "none", Some("w1:p1")),
            ])),
            "local",
        );
        transitions.observe(&quiet);
        let asking = project(
            &rest(json!([
                row("w1:p1", "working", "none", None),
                row("w1:p2", "working", "question", Some("w1:p1")),
            ])),
            "local",
        );
        let (notices, _) = transitions.observe(&asking);
        assert!(notices.is_empty());
    }

    #[test]
    fn read_root_and_child_questions_never_reannounce_the_root() {
        let mut transitions = Transitions::default();
        transitions.observe(&project(
            &rest(json!([row("w1:p1", "needs_you", "question", None),])),
            "local",
        ));
        let read = project(
            &rest(json!([row("w1:p1", "seen", "question", None),])),
            "local",
        );
        let (notices, cleared) = transitions.observe(&read);
        assert!(notices.is_empty());
        assert_eq!(
            cleared
                .iter()
                .map(|key| key.pane_id.as_str())
                .collect::<Vec<_>>(),
            ["w1:p1"]
        );

        let child = project(
            &rest(json!([
                row("w1:p1", "seen", "question", None),
                row("w1:p2", "seen", "question", Some("w1:p1")),
            ])),
            "local",
        );
        let (notices, cleared) = transitions.observe(&child);
        assert!(cleared.is_empty());
        assert!(notices.is_empty());
        assert!(transitions.observe(&child).0.is_empty());
    }

    #[test]
    fn escalations_notify_the_child_once_and_clear_when_the_parent_can_handle_it() {
        for cause in [
            "parent_blocked",
            "draft",
            "bell_exhausted",
            "child_blocked",
            "undelivered",
            "observer_unconfirmed",
        ] {
            let mut transitions = Transitions::default();
            let parent = row("w1:p1", "working", "none", None);
            let child = row("w1:p2", "seen", "question", Some("w1:p1"));
            let quiet = project(&rest(json!([parent.clone(), child.clone()])), "local");
            transitions.observe(&quiet);
            let mut raised = child.clone();
            raised["group"] = json!("needs_you");
            let human_notice = matches!(cause, "undelivered" | "observer_unconfirmed");
            raised["escalation"] = json!({"cause": cause, "human_notice": human_notice});
            let raised = project(&rest(json!([parent, raised])), "local");
            let (notices, _) = transitions.observe(&raised);
            if human_notice {
                assert!(notices.is_empty(), "{cause} already has a delivery notice");
            } else {
                assert_eq!(notices.len(), 1, "{cause}");
                assert_eq!(notices[0].key.pane_id, "w1:p2");
                assert_eq!(notices[0].state, NoticeState::NeedsYou);
            }
            assert!(transitions.observe(&raised).0.is_empty());
            let (notices, cleared) = transitions.observe(&quiet);
            assert!(notices.is_empty());
            assert_eq!(cleared.len(), usize::from(!human_notice), "{cause}");
            if !human_notice {
                assert_eq!(cleared.first().unwrap().pane_id, "w1:p2");
                assert_eq!(
                    transitions.observe(&raised).0.len(),
                    1,
                    "a new escalation is a new transition"
                );
            }
        }
    }

    #[test]
    fn an_agent_that_comes_back_in_the_same_state_is_not_announced_again() {
        let mut transitions = Transitions::default();
        let start = std::time::Instant::now();
        let asking = project(
            &rest(json!([row("w1:p1", "needs_you", "question", None)])),
            "local",
        );
        let empty = project(&rest(json!([])), "local");
        transitions.observe_at(&asking, start);
        assert_eq!(
            transitions.observe_at(&empty, start),
            (Vec::new(), BTreeSet::new()),
            "a brief absence closes nothing"
        );
        assert!(
            transitions.observe_at(&asking, start).0.is_empty(),
            "the same request after a reconnect is no new notice"
        );
        transitions.observe_at(&empty, start);
        let (_, cleared) = transitions.observe_at(&empty, start + VANISHED_TTL);
        assert_eq!(
            cleared
                .into_iter()
                .map(|key| key.pane_id)
                .collect::<Vec<_>>(),
            ["w1:p1"],
            "gone for good, its notification closes"
        );
    }

    #[test]
    fn the_mode_decides_by_renderer_count() {
        assert!(!mode_allows(PushMode::Off, 0));
        assert!(mode_allows(PushMode::AppClosed, 0));
        assert!(!mode_allows(PushMode::AppClosed, 1));
        assert!(mode_allows(PushMode::Always, 3));
    }
}
