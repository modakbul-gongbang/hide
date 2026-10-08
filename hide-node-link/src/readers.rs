//! Reader-aware boundaries for provider-bearing lists and mixed batches.
//! Provider names are checked as strings before a native format enum is read.

use std::time::Duration;

use hide_session::links::{Candidate, ReadAnswer, ReadRequest};
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::link::{LinkAnswer, LinkError, NodeLink, check_reader_call};
use crate::protocol::Call;
use crate::sessions::{ReaderFeature, ReaderFeatures};
use crate::{ErrorCode, HostError};

/// Pagination uses the original page, including refused rows, so an unknown
/// provider cannot hide an older supported session by shortening the page.
pub struct ReaderPage<T> {
    pub rows: Vec<T>,
    pub scanned: usize,
    pub oldest_unix_ms: Option<u64>,
    pub refused: usize,
}

fn facts(link: &(impl NodeLink + ?Sized)) -> Result<&ReaderFeatures, LinkError> {
    if let Some(reason) = link.closed_reason() {
        return Err(LinkError::NotConnected(reason));
    }
    link.reader_features().ok_or_else(|| {
        LinkError::Refused(HostError::new(
            ErrorCode::Unsupported,
            "session_reader_support_unavailable",
        ))
    })
}

fn shape() -> LinkError {
    LinkError::Unknown("session_reader_answer_invalid".to_owned())
}

/// Captures at most `limit` raw rows, without materializing untrusted
/// conversation bodies or deserializing their provider enums.
fn rows(answer: LinkAnswer, limit: usize) -> Result<Vec<Box<RawValue>>, LinkError> {
    struct Rows(usize);
    impl<'de> serde::de::Visitor<'de> for Rows {
        type Value = Vec<Box<RawValue>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a bounded reader page")
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut sequence: A,
        ) -> Result<Self::Value, A::Error> {
            let mut rows = Vec::new();
            while let Some(row) = sequence.next_element()? {
                if rows.len() == self.0 {
                    return Err(serde::de::Error::custom("reader page capacity"));
                }
                rows.push(row);
            }
            Ok(rows)
        }
    }
    impl<'de> serde::de::DeserializeSeed<'de> for Rows {
        type Value = Vec<Box<RawValue>>;

        fn deserialize<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_seq(self)
        }
    }
    use serde::de::DeserializeSeed;
    let raw = match answer {
        LinkAnswer::Raw(raw) => raw,
        LinkAnswer::Parsed(value) => {
            serde_json::value::to_raw_value(&value).map_err(|_| shape())?
        }
    };
    let mut deserializer = serde_json::Deserializer::from_str(raw.get());
    let rows = Rows(limit)
        .deserialize(&mut deserializer)
        .map_err(|_| shape())?;
    deserializer.end().map_err(|_| shape())?;
    Ok(rows)
}

#[derive(Deserialize)]
struct Header<'a> {
    #[serde(borrow)]
    agent: &'a str,
    #[serde(default)]
    modified_unix_ms: Option<u64>,
}

pub fn link_files(
    link: &(impl NodeLink + ?Sized),
    since_unix_ms: u64,
    until_unix_ms: Option<u64>,
    timeout: Duration,
) -> Result<ReaderPage<Candidate>, LinkError> {
    facts(link)?;
    let answer = link.call(
        Call::LinkFiles {
            since_unix_ms,
            until_unix_ms,
        },
        timeout,
    )?;
    let features = facts(link)?;
    let raw = rows(answer, hide_session::links::CANDIDATE_LIMIT)?;
    let mut page = ReaderPage {
        rows: Vec::new(),
        scanned: raw.len(),
        oldest_unix_ms: None,
        refused: 0,
    };
    for row in raw {
        let header: Header<'_> = serde_json::from_str(row.get()).map_err(|_| shape())?;
        let modified = header.modified_unix_ms.ok_or_else(shape)?;
        if modified < since_unix_ms || until_unix_ms.is_some_and(|until| modified > until) {
            return Err(shape());
        }
        page.oldest_unix_ms = Some(
            page.oldest_unix_ms
                .map_or(modified, |oldest| oldest.min(modified)),
        );
        if !features.supports(header.agent, ReaderFeature::Links) {
            page.refused += 1;
            continue;
        }
        match serde_json::from_str(row.get()) {
            Ok(candidate) => page.rows.push(candidate),
            Err(_) => page.refused += 1,
        }
    }
    Ok(page)
}

pub fn project_sessions(
    link: &(impl NodeLink + ?Sized),
    project: hide_project::ProjectIdentity,
    timeout: Duration,
) -> Result<ReaderPage<hide_session::ProjectSession>, LinkError> {
    facts(link)?;
    let answer = link.call(Call::ProjectSessions { project }, timeout)?;
    let features = facts(link)?;
    let raw = rows(answer, hide_session::SESSION_DISCOVERY_LIMIT)?;
    let mut page = ReaderPage {
        rows: Vec::new(),
        scanned: raw.len(),
        oldest_unix_ms: None,
        refused: 0,
    };
    for row in raw {
        let header: Header<'_> = match serde_json::from_str(row.get()) {
            Ok(header) => header,
            Err(_) => {
                page.refused += 1;
                continue;
            }
        };
        if !features.supports(header.agent, ReaderFeature::Identity) {
            page.refused += 1;
            continue;
        }
        let row = if features.supports(header.agent, ReaderFeature::Titles) {
            row
        } else {
            let mut fields: crate::link::RawReaderFields = match serde_json::from_str(row.get()) {
                Ok(fields) => fields,
                Err(_) => {
                    page.refused += 1;
                    continue;
                }
            };
            if fields
                .0
                .remove("title")
                .is_some_and(|raw| raw.get() != "null")
            {
                page.refused += 1;
            }
            serde_json::value::to_raw_value(&fields.0).map_err(|_| shape())?
        };
        match serde_json::from_str::<hide_session::ProjectSession>(row.get()) {
            Ok(session) => {
                page.rows.push(session);
            }
            Err(_) => page.refused += 1,
        }
    }
    Ok(page)
}

pub enum ReaderRefusalReason {
    Unsupported,
    InvalidAnswer,
}

impl ReaderRefusalReason {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unsupported => "session_reader_unsupported",
            Self::InvalidAnswer => "session_reader_answer_invalid",
        }
    }
}

pub struct ReaderRefusal {
    pub agent: hide_session::Agent,
    pub path: String,
    pub reason: ReaderRefusalReason,
}

/// Connection/reader refusals never become native file answers or advance a
/// persisted cursor. Working members of a mixed batch remain independently usable.
pub struct ReaderBatch {
    pub answers: Vec<ReadAnswer>,
    pub refused: Vec<ReaderRefusal>,
    pub ignored_answers: usize,
}

pub fn links(
    link: &(impl NodeLink + ?Sized),
    requests: Vec<ReadRequest>,
    timeout: Duration,
) -> Result<ReaderBatch, LinkError> {
    if requests.len() > hide_session::links::READ_FILE_LIMIT {
        return Err(LinkError::Refused(HostError::new(
            ErrorCode::TooLarge,
            "session_reader_batch_capacity",
        )));
    }
    let features = facts(link)?;
    let supported: Vec<_> = requests
        .iter()
        .filter(|request| features.supports(request.agent.as_str(), ReaderFeature::Links))
        .cloned()
        .collect();
    let mut ignored_answers = 0;
    let mut answers = if supported.is_empty() {
        Vec::new()
    } else {
        let call = Call::LinkRead {
            requests: supported.clone(),
        };
        check_reader_call(link, &call)?;
        let answer = link.call(call, timeout)?;
        let features = facts(link)?;
        #[derive(Deserialize)]
        struct ReadHeader<'a> {
            agent: &'a str,
            path: &'a str,
        }
        let mut answers: Vec<ReadAnswer> = Vec::new();
        for row in rows(answer, hide_session::links::READ_FILE_LIMIT)? {
            let header: ReadHeader<'_> = match serde_json::from_str(row.get()) {
                Ok(header) => header,
                Err(_) => {
                    ignored_answers += 1;
                    continue;
                }
            };
            if !features.supports(header.agent, ReaderFeature::Links)
                || !supported.iter().any(|request| {
                    request.agent.as_str() == header.agent && request.path == header.path
                })
            {
                ignored_answers += 1;
                continue;
            }
            if let Ok(answer) = serde_json::from_str(row.get()) {
                answers.push(answer);
            } else {
                ignored_answers += 1;
            }
        }
        answers
    };
    let features = facts(link)?;
    let mut result = ReaderBatch {
        answers: Vec::new(),
        refused: Vec::new(),
        ignored_answers,
    };
    for request in requests {
        if !features.supports(request.agent.as_str(), ReaderFeature::Links) {
            result.refused.push(ReaderRefusal {
                agent: request.agent,
                path: request.path,
                reason: ReaderRefusalReason::Unsupported,
            });
            continue;
        }
        let matching: Vec<_> = answers
            .iter()
            .enumerate()
            .filter(|(_, answer)| answer.agent == request.agent && answer.path == request.path)
            .map(|(index, _)| index)
            .collect();
        if matching.len() != 1 {
            result.refused.push(ReaderRefusal {
                agent: request.agent,
                path: request.path,
                reason: ReaderRefusalReason::InvalidAnswer,
            });
            continue;
        }
        result.answers.push(answers.swap_remove(matching[0]));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    // A device protocol boundary with fixed wire responses, including values
    // an older core cannot deserialize. No owned parser or store is replaced.
    struct Peer {
        features: ReaderFeatures,
        response: String,
        retire_after_answer: bool,
        retired: AtomicBool,
    }

    impl Peer {
        fn new(response: String) -> Self {
            Self {
                features: serde_json::from_str(
                    r#"[{"provider":"claude","features":["links","identity"]}]"#,
                )
                .unwrap(),
                response,
                retire_after_answer: false,
                retired: AtomicBool::new(false),
            }
        }
    }

    impl NodeLink for Peer {
        fn call(&self, call: Call, _timeout: Duration) -> Result<LinkAnswer, LinkError> {
            if let Call::LinkRead { requests } = call
                && requests
                    .iter()
                    .any(|request| request.agent != hide_session::Agent::Claude)
            {
                return Err(LinkError::Refused(HostError::new(
                    ErrorCode::InvalidRequest,
                    "unknown reader was sent",
                )));
            }
            self.retired
                .store(self.retire_after_answer, Ordering::SeqCst);
            Ok(LinkAnswer::Raw(
                RawValue::from_string(self.response.clone()).unwrap(),
            ))
        }

        fn reader_features(&self) -> Option<&ReaderFeatures> {
            (!self.retired.load(Ordering::SeqCst)).then_some(&self.features)
        }

        fn closed_reason(&self) -> Option<String> {
            self.retired
                .load(Ordering::SeqCst)
                .then(|| "replaced".to_owned())
        }
    }

    #[test]
    fn unadvertised_title_format_keeps_the_discovered_session_identity() {
        let peer = Peer::new(r#"[
            {"agent":"future-reader","title":{"future":true}},
            {"id":"native-working","agent":"claude","locator":"session.jsonl","checkout_path":"/work/app","first_human_request":null,"started_at_unix_ms":1,"updated_at_unix_ms":2,"title":{"future":true},"event_count":1,"availability":"available"}
        ]"#.to_owned());
        let page = project_sessions(
            &peer,
            hide_project::ProjectIdentity {
                id: "project".to_owned(),
                root: "/work/app".into(),
                checkout_root: "/work/app".into(),
                device_id: "fixture".to_owned(),
                kind: hide_project::ProjectKind::Folder,
            },
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(page.scanned, 2);
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].id, "native-working");
        assert!(page.rows[0].title.is_none());
        assert_eq!(page.refused, 2);
    }

    #[test]
    fn mixed_reader_batch_keeps_the_working_session_and_refuses_only_its_peer() {
        let peer = Peer::new(r#"[
            {"agent":"future-reader","path":"future"},
            {"agent":"claude","path":"working","has_more":false,"rescanned":false,"facts":{"session_id":"native-working","cwd":null,"interactive":null,"subagent":false,"first_parent_uuid":null,"last_uuid":null,"forked_from":null,"spans":[],"prs":[],"first_at_unix_ms":null,"last_at_unix_ms":null,"last_request":null}}
        ]"#.to_owned());
        let request = |agent, path: &str| ReadRequest {
            agent,
            path: path.to_owned(),
            checkpoint: None,
        };
        let batch = links(
            &peer,
            vec![
                request(hide_session::Agent::Codex, "unsupported"),
                request(hide_session::Agent::Claude, "working"),
            ],
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(batch.answers.len(), 1);
        assert_eq!(
            batch.answers[0].facts.session_id.as_deref(),
            Some("native-working")
        );
        assert_eq!(batch.refused.len(), 1);
        assert_eq!(batch.ignored_answers, 1);
        assert_eq!(batch.refused[0].path, "unsupported");
        assert!(matches!(
            batch.refused[0].reason,
            ReaderRefusalReason::Unsupported
        ));
    }

    #[test]
    fn an_unknown_reader_does_not_shorten_the_original_listing_page() {
        let unknown = r#"{"agent":"future-reader","modified_unix_ms":7}"#;
        let known = r#"{"agent":"claude","path":"working","stamp":"s","modified_unix_ms":8}"#;
        let mut response = vec![unknown; hide_session::links::CANDIDATE_LIMIT - 1];
        response.insert(0, known);
        let page = link_files(
            &Peer::new(format!("[{}]", response.join(","))),
            0,
            None,
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(page.scanned, hide_session::links::CANDIDATE_LIMIT);
        assert_eq!(page.oldest_unix_ms, Some(7));
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].path, "working");
        assert_eq!(page.refused, hide_session::links::CANDIDATE_LIMIT - 1);
        response.push(known);
        assert!(
            link_files(
                &Peer::new(format!("[{}]", response.join(","))),
                0,
                None,
                Duration::from_secs(1)
            )
            .is_err()
        );
    }

    #[test]
    fn a_replaced_connection_cannot_return_its_old_listing_or_checkpoint() {
        let mut peer = Peer::new("[]".to_owned());
        peer.retire_after_answer = true;
        assert!(matches!(
            link_files(&peer, 0, None, Duration::from_secs(1)),
            Err(LinkError::NotConnected(_))
        ));
        peer.retired.store(false, Ordering::SeqCst);
        let request = ReadRequest {
            agent: hide_session::Agent::Claude,
            path: "working".to_owned(),
            checkpoint: None,
        };
        assert!(matches!(
            links(&peer, vec![request], Duration::from_secs(1)),
            Err(LinkError::NotConnected(_))
        ));
    }
}
