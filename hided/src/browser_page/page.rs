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
pub const STEP: Duration = Duration::from_secs(8);
/// A page whose main thread answers nothing this long is held by a dialog.
const PROBE: Duration = Duration::from_secs(2);
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

pub struct Page {
    pub cdp: Cdp,
    pub display: String,
    pub top: String,
    /// Whether the display is its area's selected View, the only one shown.
    selected: bool,
    frames: Option<Vec<Frame>>,
    /// Sessions attach events have named, in attach order, not yet tagged.
    pending: Vec<(String, String, String)>,
}

/// What a snapshot of every frame read: the top document's text and one
/// section per cross-origin frame, each already in its displayed form.
pub struct Composite {
    pub top: String,
    pub sections: Vec<(usize, String)>,
}

impl Composite {
    pub fn text(&self) -> String {
        if self.sections.is_empty() {
            return self.top.clone();
        }
        let sections: Vec<&str> = self
            .sections
            .iter()
            .map(|(_, text)| text.as_str())
            .collect();
        format!("{}\n\n{}\n", self.top.trim_end(), sections.join("\n\n"))
    }
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
        // An open JavaScript dialog holds the renderer: nothing on a new
        // session answers, and the dialog event is not sent again.
        let top = page.top.clone();
        match page
            .cdp
            .call("Page.enable", json!({}), Some(&top), PROBE)
            .await
        {
            Ok(_) => Ok(page),
            Err(CdpError::Timeout) => Err(Failure::new("dialog_open", None)),
            Err(error) => Err(page.failure(error)),
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
                crate::browser_relay::CLOSE_IDLE => Failure::new("browser_relay_idle", None),
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
            let (session, target, parent) = self.pending.remove(0);
            if frames.len() >= MAX_FRAMES || detached.contains(&session) {
                continue;
            }
            // A frame that navigated or closed while attaching is no longer
            // part of the page.
            let Ok(tag) = self.dom(&session, "tag", json!({})).await else {
                continue;
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
                self.pending
                    .push((session.to_owned(), target.to_owned(), parent.to_owned()));
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
        for (index, frame) in self.frames().await?.into_iter().enumerate() {
            let Ok(text) = self
                .eval(
                    &frame.session,
                    &format!(
                        "({SNAPSHOT_JS})({},{},true)",
                        json!(filter),
                        json!(clickable)
                    ),
                )
                .await
            else {
                continue;
            };
            let body = section_body(text.as_str().unwrap_or(""), &frame.tag);
            if body.is_empty() {
                continue;
            }
            sections.push((
                index,
                format!("# OOPIF {} origin={}\n{body}", frame.tag, frame.origin),
            ));
        }
        Ok(Composite {
            top: top_text,
            sections,
        })
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
        let frames = self.frames().await?;
        let mut previous_sections = Vec::new();
        for (index, section) in &composite.sections {
            let session = frames[*index].session.clone();
            if let Ok(answer) = self
                .dom(&session, "baseline", json!({"key": key, "text": section}))
                .await
                && let Some(previous) = answer["previous"].as_str()
            {
                previous_sections.push(previous.to_owned());
            }
        }
        Ok(previous_top.map(|top| {
            Composite {
                top,
                sections: previous_sections
                    .into_iter()
                    .map(|text| (0, text))
                    .collect(),
            }
            .text()
        }))
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
            sections: vec![(0, "# OOPIF k7q2 origin=http://b\n@k7q2:1 link \"B\"".into())],
        };
        assert_eq!(
            composite.text(),
            "# T\n# http://a/\n\n@1 button \"A\"\n\n# OOPIF k7q2 origin=http://b\n@k7q2:1 link \"B\"\n"
        );
    }
}
