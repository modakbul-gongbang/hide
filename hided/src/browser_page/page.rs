//! One display for the life of one command: the relay connection, the
//! display's page session, the cross-origin frames auto-attached under it,
//! and the page-side assets evaluated in each.

use std::time::Duration;

use serde_json::{Value, json};

use super::Failure;
use super::cdp::{Cdp, CdpError, Event};

pub const SNAPSHOT_JS: &str = include_str!("../../assets/browser/snapshot.js");
pub const DOM_JS: &str = include_str!("../../assets/browser/dom.js");
pub const RENDER_JS: &str = include_str!("../../assets/browser/render.js");
pub const OVERLAY_JS: &str = include_str!("../../assets/browser/overlay.js");

/// Below the gateway's ten-second command deadline, which would otherwise
/// release the whole debugger lease mid-command.
#[cfg(not(test))]
pub const STEP: Duration = Duration::from_secs(8);
/// A test that lets a step lapse waits this long, not the real step.
#[cfg(test)]
pub const STEP: Duration = Duration::from_millis(400);
const OVERLAY: Duration = Duration::from_secs(1);
/// Nested cross-origin frames a snapshot follows; the gateway admits 64
/// sessions per client.
const MAX_FRAMES: usize = 24;

/// A cross-origin frame: its own session, the session of the document that
/// embeds it, and the tag its document carries for `@<tag>:N` refs.
#[derive(Debug, Clone)]
pub struct Frame {
    pub session: String,
    pub target: String,
    pub parent: String,
    pub tag: String,
    pub origin: String,
}

/// A cross-origin frame that answered nothing within a step. Its document
/// is out of reach, so it has no tag, no refs and no baseline: the snapshot
/// only says it is there.
#[derive(Debug, Clone)]
struct Silent {
    session: String,
    origin: String,
}

impl Silent {
    fn note(&self) -> String {
        format!(
            "# OOPIF unresponsive origin={} - no answer in time; a script that never yields, or a dialog the operator has not answered",
            self.origin
        )
    }
}

/// A session an attach event named that has not been read yet: the frame's
/// session, its target and its parent's session, and the address it was
/// attached at.
struct Attached {
    session: String,
    target: String,
    parent: String,
    url: String,
}

pub struct Page {
    pub cdp: Cdp,
    pub display: String,
    pub top: String,
    /// Whether the display is its area's selected View, the only one shown.
    selected: bool,
    frames: Option<Vec<Frame>>,
    silent: Vec<Silent>,
    /// Sessions attach events have named, in attach order, not yet tagged.
    pending: Vec<Attached>,
}

/// What a snapshot of every frame read: the top document's text, one
/// section per cross-origin frame with the session it came from, each
/// already in its displayed form, and a note for each frame that did not
/// answer.
pub struct Composite {
    pub top: String,
    pub sections: Vec<(String, String)>,
    pub silent: Vec<String>,
}

impl Composite {
    pub fn text(&self) -> String {
        let mut sections: Vec<String> =
            self.sections.iter().map(|(_, text)| text.clone()).collect();
        sections.extend(self.silent.iter().cloned());
        compose(&self.top, &sections)
    }
}

/// The origin of an address a page attached a frame at; `opaque` for one
/// that has none (`about:blank`, `data:`) or is not an `http(s)` address.
fn origin_of(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "opaque".to_owned();
    };
    if !matches!(scheme, "http" | "https") {
        return "opaque".to_owned();
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    if host.is_empty() {
        return "opaque".to_owned();
    }
    format!("{scheme}://{host}")
}

/// The displayed text of a top document and its frames' sections.
fn compose(top: &str, sections: &[String]) -> String {
    if sections.is_empty() {
        return top.to_owned();
    }
    format!("{}\n\n{}\n", top.trim_end(), sections.join("\n\n"))
}

/// The four characters `dom.js` draws a frame tag from.
fn valid_tag(tag: &str) -> bool {
    tag.len() == 4
        && tag
            .bytes()
            .all(|b| b"abcdefghijkmnpqrstuvwxyz23456789".contains(&b))
}

pub fn call(asset: &str, op: &str, args: &Value) -> String {
    format!("({asset})({},{args})", json!(op))
}

impl Page {
    pub async fn attach(cdp: Cdp, display: &str, selected: bool) -> Result<Self, Failure> {
        let mut page = Self {
            cdp,
            display: display.to_owned(),
            top: String::new(),
            selected,
            frames: None,
            silent: Vec::new(),
            pending: Vec::new(),
        };
        let targets = page
            .cdp
            .call("Target.getTargets", json!({}), None, STEP)
            .await
            .map_err(|error| page.failure(error))?;
        let target = targets["targetInfos"]
            .as_array()
            .and_then(|targets| targets.iter().find(|target| target["type"] == "page"))
            .and_then(|target| target["targetId"].as_str())
            .ok_or_else(|| Failure::new("display_missing", None))?
            .to_owned();
        let attached = page
            .cdp
            .call(
                "Target.attachToTarget",
                json!({"targetId": target, "flatten": true}),
                None,
                STEP,
            )
            .await;
        page.top = match attached {
            Ok(answer) => answer["sessionId"]
                .as_str()
                .ok_or_else(|| Failure::new("display_missing", None))?
                .to_owned(),
            Err(CdpError::Protocol(message)) if message.contains("already has a debugger") => {
                return Err(Failure::new("display_busy", None));
            }
            Err(error) => return Err(page.failure(error)),
        };
        let top = page.top.clone();
        page.enable(&top).await?;
        Ok(page)
    }

    /// Turns on the dialog events of the top document. An open JavaScript
    /// dialog holds the renderer, so nothing on a new session answers and the
    /// dialog event is not sent again; a top document silent for a whole step
    /// is taken to be held by one. A cross-origin frame has its own renderer,
    /// so its silence is only its own (`frames`).
    async fn enable(&mut self, session: &str) -> Result<(), Failure> {
        match self
            .cdp
            .call("Page.enable", json!({}), Some(session), STEP)
            .await
        {
            Ok(_) => Ok(()),
            Err(CdpError::Timeout) => Err(Failure::new("dialog_open", None)),
            Err(error) => Err(self.failure(error)),
        }
    }

    /// The reason a transport or protocol error stands for.
    pub fn failure(&self, error: CdpError) -> Failure {
        match error {
            CdpError::Timeout => Failure::new("page_unresponsive", None),
            CdpError::Protocol(message) => Failure::new("cdp_error", Some(message)),
            CdpError::Closed { code, reason } => match code {
                crate::browser_relay::CLOSE_MESSAGE_LIMIT => {
                    Failure::new("browser_relay_message_limit", None)
                }
                // The relay saw no answer for a minute, or outlived any command.
                crate::browser_relay::CLOSE_IDLE => {
                    Failure::new("page_unresponsive", (!reason.is_empty()).then_some(reason))
                }
                crate::browser_relay::CLOSE_GATEWAY_LOST => {
                    Failure::new("browser_control_unavailable", None)
                }
                // The gateway's resource limit (an oversized reply).
                1013 => Failure::new("browser_limit", Some(reason)),
                _ => Failure::new("display_closed", (!reason.is_empty()).then_some(reason)),
            },
        }
    }

    /// Evaluates a page-side expression and returns its value; an asset that
    /// returns `{error}` becomes that reason.
    pub async fn eval(&mut self, session: &str, expression: &str) -> Result<Value, Failure> {
        let answer = self
            .cdp
            .call(
                "Runtime.evaluate",
                json!({"expression": expression, "returnByValue": true}),
                Some(session),
                STEP,
            )
            .await
            .map_err(|error| self.blocked(error))?;
        if let Some(details) = answer.get("exceptionDetails") {
            return Err(Failure::new(
                "page_script_failed",
                Some(first_line(details)),
            ));
        }
        let value = answer["result"]
            .get("value")
            .cloned()
            .unwrap_or(Value::Null);
        if let Some(reason) = value.get("error").and_then(Value::as_str) {
            return Err(Failure::new(
                reason,
                value["detail"].as_str().map(str::to_owned),
            ));
        }
        Ok(value)
    }

    /// A step that times out after the attach probe answered is held by a
    /// dialog the page opened meanwhile, or by a script that never yields.
    pub fn blocked(&self, error: CdpError) -> Failure {
        match error {
            CdpError::Timeout if self.cdp.dialog.is_some() => Failure::new("dialog_open", None),
            error => self.failure(error),
        }
    }

    pub async fn dom(&mut self, session: &str, op: &str, args: Value) -> Result<Value, Failure> {
        self.eval(session, &call(DOM_JS, op, &args)).await
    }

    pub async fn probe(&mut self) -> Result<Value, Failure> {
        let top = self.top.clone();
        self.dom(&top, "probe", json!({})).await
    }

    /// Input and pixels need a display on screen: a View behind another
    /// tab never answers input, and would take it later, unseen. The page's
    /// own visibility cannot tell: a selected View in a window covered by
    /// another app reads hidden and still takes input.
    pub async fn require_visible(&mut self) -> Result<Value, Failure> {
        if !self.selected {
            return Err(Failure::new("display_hidden", None));
        }
        self.probe().await
    }

    /// Draws on the operator's view of the page. It never fails a command.
    pub async fn overlay(&mut self, op: &str, args: Value) -> Option<Value> {
        if self.cdp.dialog.is_some() {
            return None;
        }
        let top = self.top.clone();
        let answer = self
            .cdp
            .call(
                "Runtime.evaluate",
                json!({"expression": call(OVERLAY_JS, op, &args), "returnByValue": true}),
                Some(&top),
                OVERLAY,
            )
            .await
            .ok()?;
        answer["result"].get("value").cloned()
    }

    /// The cross-origin frames under the page, found by auto-attach and
    /// tagged on first use. Nested frames attach under their parent frame.
    /// Later calls fold in frames that attached or detached since (a wait).
    pub async fn frames(&mut self) -> Result<Vec<Frame>, Failure> {
        let mut parents = Vec::new();
        let mut frames = match self.frames.take() {
            Some(frames) => frames,
            None => {
                parents.push(self.top.clone());
                Vec::new()
            }
        };
        let detached: Vec<String> = self
            .cdp
            .take_events(|event| event.method == "Target.detachedFromTarget")
            .into_iter()
            .filter_map(|event| event.params["sessionId"].as_str().map(str::to_owned))
            .collect();
        frames.retain(|frame| !detached.contains(&frame.session));
        self.silent
            .retain(|frame| !detached.contains(&frame.session));
        loop {
            if let Some(parent) = parents.pop() {
                self.cdp
                    .call(
                        "Target.setAutoAttach",
                        json!({"autoAttach": true, "waitForDebuggerOnStart": false, "flatten": true,
                            "filter": [{"type": "iframe", "exclude": false}, {"exclude": true}]}),
                        Some(&parent),
                        STEP,
                    )
                    .await
                    .map_err(|error| self.blocked(error))?;
                self.flush(&parent).await?;
            }
            self.collect_attached();
            if self.pending.is_empty() {
                if parents.is_empty() {
                    break;
                }
                continue;
            }
            let Attached {
                session,
                target,
                parent,
                url,
            } = self.pending.remove(0);
            if frames.len() + self.silent.len() >= MAX_FRAMES || detached.contains(&session) {
                continue;
            }
            // A frame that navigated or closed while attaching is no longer
            // part of the page. A frame that answers nothing is held, by a
            // script or a dialog, in its own renderer; only a dialog event
            // this command saw is a dialog the operator must answer.
            let tag = match self
                .cdp
                .call("Page.enable", json!({}), Some(&session), STEP)
                .await
            {
                Ok(_) => self.dom(&session, "tag", json!({})).await,
                Err(error) => Err(self.blocked(error)),
            };
            let tag = match tag {
                Ok(tag) => tag,
                Err(failure) if failure.reason == "page_unresponsive" => {
                    self.silent.push(Silent {
                        session,
                        origin: origin_of(&url),
                    });
                    continue;
                }
                Err(failure) if failure.page_side() => continue,
                Err(failure) => return Err(failure),
            };
            parents.push(session.clone());
            frames.push(Frame {
                session,
                target,
                parent,
                tag: tag["tag"].as_str().unwrap_or("").to_owned(),
                origin: tag["origin"].as_str().unwrap_or("opaque").to_owned(),
            });
        }
        // The tag lives in the frame's own document, so a hostile frame can
        // set any: a malformed one, or one that two frames share, names no
        // frame, and refs never reach a frame by guesswork.
        let tags: Vec<String> = frames.iter().map(|frame| frame.tag.clone()).collect();
        frames.retain(|frame| {
            valid_tag(&frame.tag) && tags.iter().filter(|tag| **tag == frame.tag).count() == 1
        });
        self.frames = Some(frames.clone());
        Ok(frames)
    }

    fn collect_attached(&mut self) {
        let attached: Vec<Event> = self
            .cdp
            .take_events(|event| event.method == "Target.attachedToTarget");
        for event in attached {
            let info = &event.params["targetInfo"];
            if info["type"] != "iframe" {
                continue;
            }
            if let (Some(session), Some(target), Some(parent)) = (
                event.params["sessionId"].as_str(),
                info["targetId"].as_str(),
                event.session.as_deref(),
            ) {
                self.pending.push(Attached {
                    session: session.to_owned(),
                    target: target.to_owned(),
                    parent: parent.to_owned(),
                    url: info["url"].as_str().unwrap_or("").to_owned(),
                });
            }
        }
    }

    /// Events sent before this round trip's answer have all arrived.
    pub async fn flush(&mut self, session: &str) -> Result<(), Failure> {
        self.cdp
            .call(
                "Runtime.evaluate",
                json!({"expression": "0", "returnByValue": true}),
                Some(session),
                STEP,
            )
            .await
            .map(|_| ())
            .map_err(|error| self.blocked(error))
    }

    /// The frame an `@<tag>:N` ref names, or `ref_stale` when no current
    /// frame document carries the tag (it navigated or closed).
    pub async fn frame(&mut self, tag: &str) -> Result<Frame, Failure> {
        self.frames()
            .await?
            .into_iter()
            .find(|frame| frame.tag == tag)
            .ok_or_else(|| Failure::new("ref_stale", Some(format!("frame {tag}"))))
    }

    /// Where a cross-origin frame's viewport starts in the top viewport, so
    /// its points can be drawn and cropped in top-page coordinates.
    pub async fn frame_offset(&mut self, frame: &Frame) -> Result<(f64, f64), Failure> {
        let frames = self.frames().await?;
        let mut offset = (0.0, 0.0);
        let mut current = frame.clone();
        loop {
            let owner = self
                .cdp
                .call(
                    "DOM.getFrameOwner",
                    json!({"frameId": current.target}),
                    Some(&current.parent),
                    STEP,
                )
                .await
                .map_err(|error| self.blocked(error))?;
            let model = self
                .cdp
                .call(
                    "DOM.getBoxModel",
                    json!({"backendNodeId": owner["backendNodeId"]}),
                    Some(&current.parent),
                    STEP,
                )
                .await
                .map_err(|error| self.blocked(error))?;
            let content = &model["model"]["content"];
            offset.0 += content[0].as_f64().unwrap_or(0.0);
            offset.1 += content[1].as_f64().unwrap_or(0.0);
            match frames.iter().find(|frame| frame.session == current.parent) {
                Some(parent) => current = parent.clone(),
                None => return Ok(offset),
            }
        }
    }

    /// Reads every frame. `baseline` names the --diff baseline kind to swap
    /// in each frame's own document; cross-origin frames are read with their
    /// values and paths reduced to origins.
    pub async fn snapshot(
        &mut self,
        filter: Option<&str>,
        clickable: &str,
    ) -> Result<Composite, Failure> {
        let top = self.top.clone();
        let text = self
            .eval(
                &top,
                &format!(
                    "({SNAPSHOT_JS})({},{},false)",
                    json!(filter),
                    json!(clickable)
                ),
            )
            .await?;
        let top_text = text.as_str().unwrap_or("").to_owned();
        let mut sections = Vec::new();
        for frame in self.frames().await? {
            let read = self
                .eval(
                    &frame.session,
                    &format!(
                        "({SNAPSHOT_JS})({},{},true)",
                        json!(filter),
                        json!(clickable)
                    ),
                )
                .await;
            // A frame that navigated mid-read has a new document, and its
            // refs a new tag; the next snapshot reads it again. One that
            // stopped answering is noted, and the rest of the page is read.
            let text = match read {
                Ok(text) => text,
                Err(failure) if failure.reason == "page_unresponsive" => {
                    self.silence(&frame);
                    continue;
                }
                Err(failure) if failure.page_side() => continue,
                Err(failure) => return Err(failure),
            };
            match self.dom(&frame.session, "tag", json!({})).await {
                Ok(current) if current["tag"] == frame.tag.as_str() => {}
                Ok(_) => continue,
                Err(failure) if failure.reason == "page_unresponsive" => {
                    self.silence(&frame);
                    continue;
                }
                Err(failure) if failure.page_side() => continue,
                Err(failure) => return Err(failure),
            }
            let body = section_body(text.as_str().unwrap_or(""), &frame.tag);
            if body.is_empty() {
                continue;
            }
            sections.push((
                frame.session,
                format!("# OOPIF {} origin={}\n{body}", frame.tag, frame.origin),
            ));
        }
        Ok(Composite {
            top: top_text,
            sections,
            silent: self.silent.iter().map(Silent::note).collect(),
        })
    }

    /// Moves a frame that stopped answering out of the frames a command
    /// reads, so the rest of the command does not wait on it again.
    fn silence(&mut self, frame: &Frame) {
        if let Some(frames) = self.frames.as_mut() {
            frames.retain(|known| known.session != frame.session);
        }
        self.silent.push(Silent {
            session: frame.session.clone(),
            origin: frame.origin.clone(),
        });
    }

    /// Swaps the composite into each frame's document as the new baseline
    /// of `key` and returns the previous composite, or none when the top
    /// document has none (a first snapshot or a new document).
    pub async fn swap_baseline(
        &mut self,
        key: &str,
        composite: &Composite,
    ) -> Result<Option<String>, Failure> {
        let top = self.top.clone();
        let previous_top = self
            .dom(&top, "baseline", json!({"key": key, "text": composite.top}))
            .await?["previous"]
            .as_str()
            .map(str::to_owned);
        let mut previous_sections = Vec::new();
        for (session, section) in &composite.sections {
            match self
                .dom(session, "baseline", json!({"key": key, "text": section}))
                .await
            {
                Ok(answer) => {
                    if let Some(previous) = answer["previous"].as_str() {
                        previous_sections.push(previous.to_owned());
                    }
                }
                Err(failure) if failure.page_side() => {}
                Err(failure) => return Err(failure),
            }
        }
        // A silent frame has no baseline: the note stands in both snapshots,
        // so a diff does not report it as new each time.
        previous_sections.extend(composite.silent.iter().cloned());
        Ok(previous_top.map(|top| compose(&top, &previous_sections)))
    }

    /// Runs render.js in a fresh isolated world of the top frame: it sees
    /// only its arguments, and page scripts cannot see them.
    pub async fn render(&mut self, op: &str, args: Value) -> Result<Value, Failure> {
        let top = self.top.clone();
        let tree = self
            .cdp
            .call("Page.getFrameTree", json!({}), Some(&top), STEP)
            .await
            .map_err(|error| self.blocked(error))?;
        let world = self
            .cdp
            .call(
                "Page.createIsolatedWorld",
                json!({"frameId": tree["frameTree"]["frame"]["id"], "worldName": "hide-browser"}),
                Some(&top),
                STEP,
            )
            .await
            .map_err(|error| self.blocked(error))?;
        let answer = self
            .cdp
            .call(
                "Runtime.evaluate",
                json!({"expression": call(RENDER_JS, op, &args), "contextId": world["executionContextId"], "returnByValue": true}),
                Some(&top),
                STEP,
            )
            .await
            .map_err(|error| self.blocked(error))?;
        if let Some(details) = answer.get("exceptionDetails") {
            return Err(Failure::new(
                "page_script_failed",
                Some(first_line(details)),
            ));
        }
        Ok(answer["result"]
            .get("value")
            .cloned()
            .unwrap_or(Value::Null))
    }

    pub async fn close(self) {
        self.cdp.close().await;
    }
}

/// A frame's snapshot without its title and address lines, with each ref
/// renamed into the frame's namespace (`@3` -> `@k7q2:3`).
pub fn section_body(text: &str, tag: &str) -> String {
    text.lines()
        .skip(2)
        .map(|line| {
            let indent = line.len() - line.trim_start_matches(' ').len();
            let rest = &line[indent..];
            match rest.strip_prefix('@') {
                Some(after)
                    if after.split_once(' ').is_some_and(|(digits, _)| {
                        !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
                    }) =>
                {
                    format!("{}@{tag}:{after}", &line[..indent])
                }
                _ => line.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

pub fn first_line(details: &Value) -> String {
    details["exception"]["description"]
        .as_str()
        .or_else(|| details["text"].as_str())
        .unwrap_or("script failed")
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(300)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_section_names_its_refs_in_its_own_namespace() {
        let text = "# Child\n# http://localhost:1/child\n\n@1 textbox \"Name\" [text]\n  @12 button \"Go\"\n  heading \"@3 is text\"\n";
        assert_eq!(
            section_body(text, "k7q2"),
            "@k7q2:1 textbox \"Name\" [text]\n  @k7q2:12 button \"Go\"\n  heading \"@3 is text\""
        );
    }

    #[test]
    fn a_composite_puts_each_frame_section_after_the_top_document() {
        let composite = Composite {
            top: "# T\n# http://a/\n\n@1 button \"A\"\n".into(),
            sections: vec![(
                "s1".into(),
                "# OOPIF k7q2 origin=http://b\n@k7q2:1 link \"B\"".into(),
            )],
            silent: Vec::new(),
        };
        assert_eq!(
            composite.text(),
            "# T\n# http://a/\n\n@1 button \"A\"\n\n# OOPIF k7q2 origin=http://b\n@k7q2:1 link \"B\"\n"
        );
    }

    /// A scripted gateway on a local socket: `script` answers each request
    /// with the messages to send back, events first, and none to stay silent.
    async fn gateway(mut script: impl FnMut(&Value) -> Vec<Value> + Send + 'static) -> Cdp {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(Ok(message)) = futures_util::StreamExt::next(&mut socket).await {
                let Ok(text) = message.into_text() else {
                    continue;
                };
                let request: Value = serde_json::from_str(text.as_str()).unwrap();
                for reply in script(&request) {
                    let reply = tokio_tungstenite::tungstenite::Message::text(reply.to_string());
                    futures_util::SinkExt::send(&mut socket, reply)
                        .await
                        .unwrap();
                }
            }
        });
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}"))
            .await
            .unwrap();
        Cdp::new(socket)
    }

    fn reply(request: &Value, result: Value) -> Value {
        let mut answer = json!({"id": request["id"], "result": result});
        if let Some(session) = request["sessionId"].as_str() {
            answer["sessionId"] = json!(session);
        }
        answer
    }

    fn attached(session: &str, parent: &str, url: &str) -> Value {
        json!({"method": "Target.attachedToTarget", "sessionId": parent,
            "params": {"sessionId": session, "targetInfo": {"type": "iframe", "targetId": format!("t-{session}"), "url": url}}})
    }

    /// A top document with two cross-origin frames: `f1` never answers
    /// `Page.enable`, `f2` is a healthy page. `before` is sent ahead of f1's
    /// silence, the way a dialog event reaches a client that listens.
    fn page_with_hung_frame(before: Option<Value>) -> impl FnMut(&Value) -> Vec<Value> + Send {
        move |request| {
            let session = request["sessionId"].as_str().unwrap_or("");
            let expression = request["params"]["expression"].as_str().unwrap_or("");
            match (request["method"].as_str().unwrap(), session) {
                ("Target.setAutoAttach", "top") => vec![
                    attached("f1", "top", "http://hung.test:8/ad"),
                    attached("f2", "top", "http://ok.test:9/child"),
                    reply(request, json!({})),
                ],
                ("Target.setAutoAttach", _) => vec![reply(request, json!({}))],
                ("Page.enable", "f1") => before.clone().into_iter().collect(),
                ("Page.enable", _) => vec![reply(request, json!({}))],
                ("Runtime.evaluate", "top") if expression == "0" => {
                    vec![reply(
                        request,
                        json!({"result": {"type": "number", "value": 0}}),
                    )]
                }
                ("Runtime.evaluate", "top") => vec![reply(
                    request,
                    json!({"result": {"value": "# T\n# http://a/\n\n@1 button \"A\"\n"}}),
                )],
                ("Runtime.evaluate", "f2") if expression.ends_with("(\"tag\",{})") => vec![reply(
                    request,
                    json!({"result": {"value": {"tag": "k7q2", "origin": "http://ok.test:9"}}}),
                )],
                ("Runtime.evaluate", "f2") => vec![reply(
                    request,
                    json!({"result": {"value": "# Child\n# http://ok.test:9/child\n\n@1 link \"B\"\n"}}),
                )],
                _ => Vec::new(),
            }
        }
    }

    async fn page(script: impl FnMut(&Value) -> Vec<Value> + Send + 'static) -> Page {
        Page {
            cdp: gateway(script).await,
            display: "browser-1".into(),
            top: "top".into(),
            selected: true,
            frames: None,
            silent: Vec::new(),
            pending: Vec::new(),
        }
    }

    #[tokio::test]
    async fn a_frame_that_answers_nothing_is_noted_and_the_rest_of_the_page_is_read() {
        let mut page = page(page_with_hung_frame(None)).await;
        let composite = page.snapshot(None, "auto").await.unwrap();
        let text = composite.text();
        assert!(text.contains("@1 button \"A\""), "{text}");
        assert!(
            text.contains("# OOPIF k7q2 origin=http://ok.test:9\n@k7q2:1 link \"B\""),
            "{text}"
        );
        assert!(
            text.contains("# OOPIF unresponsive origin=http://hung.test:8 - no answer in time"),
            "{text}"
        );
        // It has no tag, so no ref reaches it and no baseline waits on it.
        assert_eq!(page.frames().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_dialog_event_in_the_same_command_still_fails_dialog_open() {
        let dialog = json!({"method": "Page.javascriptDialogOpening", "sessionId": "f1",
            "params": {"type": "alert", "message": "Hello"}});
        let mut page = page(page_with_hung_frame(Some(dialog))).await;
        let failure = page.snapshot(None, "auto").await.err().unwrap();
        assert_eq!(failure.reason, "dialog_open");
    }

    #[tokio::test]
    async fn a_top_document_that_answers_nothing_still_fails_dialog_open() {
        let cdp = gateway(|request| match request["method"].as_str().unwrap() {
            "Target.getTargets" => vec![reply(
                request,
                json!({"targetInfos": [{"type": "page", "targetId": "p1"}]}),
            )],
            "Target.attachToTarget" => vec![reply(request, json!({"sessionId": "top"}))],
            _ => Vec::new(),
        })
        .await;
        let failure = Page::attach(cdp, "browser-1", true).await.err().unwrap();
        assert_eq!(failure.reason, "dialog_open");
    }

    #[test]
    fn a_frames_origin_comes_from_the_address_it_attached_at() {
        assert_eq!(
            origin_of("https://user:pw@ads.test:8443/a?b#c"),
            "https://ads.test:8443"
        );
        assert_eq!(origin_of("http://localhost:3/"), "http://localhost:3");
        for url in [
            "",
            "about:blank",
            "data:text/html,x",
            "file:///etc/passwd",
            "http:///x",
        ] {
            assert_eq!(origin_of(url), "opaque", "{url}");
        }
    }
}
