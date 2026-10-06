//! The `hide factory` command contract: what each command carries over the
//! pane-capability socket, which permission it needs, and how its answer is
//! printed for a person (`--json` prints the answer as it is).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{CheckPoint, DiscoveryClass, MergeMode, Runtime};
use crate::role::Permission;

/// A new or updated card as `hide factory add` sends it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CardInput {
    pub title: Option<String>,
    pub goal: Option<String>,
    pub criteria: Vec<String>,
    pub out_of_scope: Vec<String>,
    pub open_decisions: Vec<String>,
    pub depends_on: Vec<String>,
    pub external: Vec<String>,
    pub review_directly: bool,
    pub priority: Option<i32>,
    pub merge_mode: Option<MergeMode>,
    pub runtime: Option<Runtime>,
    /// A PRD the daemon copies into the Factory's private folder (D-10).
    pub prd: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Without `confirm`, shows what would be created and writes nothing (B1).
    Init {
        project: String,
        verification: Option<VerificationChoice>,
        merge_mode: Option<MergeMode>,
        confirm: bool,
    },
    /// New Task, or the same Task again by `task` or `issue` (B9, B14).
    Add {
        project: Option<String>,
        task: Option<String>,
        issue: Option<String>,
        card: CardInput,
        /// The adding pane, where a pending review's result goes (B11).
        producer_pane: Option<String>,
    },
    Status {
        project: Option<String>,
    },
    Show {
        task: String,
    },
    Inbox,
    Answer {
        task: String,
        question: Option<String>,
        /// `suggestion`, `default`, a listed choice, or free text.
        choice: Option<String>,
        text: Option<String>,
    },
    Ask {
        text: String,
        suggestion: String,
        default_action: String,
        deadline_hours: Option<u64>,
        letter: Option<String>,
    },
    Block {
        text: String,
        suggestion: String,
        deadline_hours: Option<u64>,
        letter: Option<String>,
    },
    Propose {
        class: DiscoveryClass,
        text: String,
        /// A prerequisite or unrelated Task's card.
        card: Option<CardInput>,
        /// An enabled autonomy scope the new Task claims (B32).
        autonomy: Option<String>,
        /// Moves an earlier discovery to `class`; only toward a person (B30).
        reclassify: Option<String>,
        letter: Option<String>,
    },
    Done {
        summary: Option<String>,
        breaking: bool,
        letter: Option<String>,
    },
    Decide {
        text: String,
    },
    Config {
        project: Option<String>,
        set: Vec<(String, String)>,
    },
    Priority {
        task: String,
        priority: i32,
    },
    Dep {
        task: String,
        on: String,
        remove: bool,
    },
    Pause {
        task: String,
    },
    Resume {
        task: String,
    },
    Retry {
        task: String,
    },
    Merge {
        task: String,
    },
    RequestChanges {
        task: String,
        comment: String,
    },
    Cancel {
        task: String,
    },
    Revive {
        task: String,
    },
    Close {
        project: Option<String>,
    },
    /// A natural-language check (B67).
    Check {
        project: Option<String>,
        at: CheckPoint,
        instruction: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerificationChoice {
    Ci { checks: Vec<String> },
    Commands { commands: Vec<String> },
    None,
}

impl Command {
    pub fn permission(&self) -> Permission {
        match self {
            Self::Status { .. } | Self::Show { .. } | Self::Inbox => Permission::Read,
            Self::Ask { .. }
            | Self::Block { .. }
            | Self::Propose { .. }
            | Self::Done { .. }
            | Self::Decide { .. } => Permission::Report,
            Self::Dep { remove: false, .. } => Permission::AddDependency,
            Self::Dep { remove: true, .. } | Self::Priority { .. } => Permission::Loosen,
            Self::Init { .. } | Self::Add { .. } | Self::Check { .. } => Permission::Intake,
            Self::Answer { .. } => Permission::Answer,
            Self::Merge { .. } | Self::RequestChanges { .. } => Permission::Merge,
            Self::Pause { .. }
            | Self::Resume { .. }
            | Self::Retry { .. }
            | Self::Cancel { .. }
            | Self::Revive { .. }
            | Self::Close { .. } => Permission::Control,
            Self::Config { set, .. } if set.is_empty() => Permission::Read,
            Self::Config { .. } => Permission::Configure,
        }
    }

    pub fn verb(&self) -> &'static str {
        match self {
            Self::Init { .. } => "init",
            Self::Add { .. } => "add",
            Self::Status { .. } => "status",
            Self::Show { .. } => "show",
            Self::Inbox => "inbox",
            Self::Answer { .. } => "answer",
            Self::Ask { .. } => "ask",
            Self::Block { .. } => "block",
            Self::Propose { .. } => "propose",
            Self::Done { .. } => "done",
            Self::Decide { .. } => "decide",
            Self::Config { .. } => "config",
            Self::Priority { .. } => "priority",
            Self::Dep { .. } => "dep",
            Self::Pause { .. } => "pause",
            Self::Resume { .. } => "resume",
            Self::Retry { .. } => "retry",
            Self::Merge { .. } => "merge",
            Self::RequestChanges { .. } => "request-changes",
            Self::Cancel { .. } => "cancel",
            Self::Revive { .. } => "revive",
            Self::Close { .. } => "close",
            Self::Check { .. } => "check",
        }
    }
}

/// A refusal: a stable code, the facts the caller needs, and the next step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub reason: String,
    pub next_action: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub detail: Value,
}

impl Refusal {
    pub fn new(reason: &str, next_action: impl Into<String>) -> Self {
        Self {
            reason: reason.to_owned(),
            next_action: next_action.into(),
            detail: Value::Null,
        }
    }

    pub fn with(mut self, detail: Value) -> Self {
        self.detail = detail;
        self
    }

    pub fn to_json(&self) -> Value {
        let mut value = serde_json::json!({
            "ok": false,
            "reason": self.reason,
            "next_action": self.next_action,
        });
        if !self.detail.is_null() {
            value["detail"] = self.detail.clone();
        }
        value
    }
}

pub const USAGE: &str = "hide factory init <project> [--ci [<check>...]] [--verify <command>]... [--no-verification] [--merge auto|manual] [--confirm]
hide factory add [--task <id>|<issue>] --title <t> --goal <g> --criterion <c>... [--out-of-scope <s>]... [--open <decision>]... [--after <task>]... [--external <ref>]... [--prd <path>] [--review-directly] [--priority <n>] [--merge auto|manual] [--runtime claude|codex] [--project <path>]
hide factory status [--project <path>]
hide factory show <task>
hide factory inbox
hide factory answer <task> [--question <id>] [--choose suggestion|default|<choice>] [--text <answer>]
hide factory ask --question <text> --suggestion <text> --default <action> [--deadline-hours <n>]
hide factory block --question <text> --suggestion <text> [--deadline-hours <n>]
hide factory propose --class in-scope|decision|scope-change|prerequisite|unrelated --text <text> [--title <t> --goal <g> --criterion <c>...] [--autonomy <scope>] [--reclassify <discovery>]
hide factory done [--summary <text>] [--breaking]
hide factory decide --text <decision>
hide factory config [--project <path>] [--set <key>=<value>]...
hide factory priority <task> <n>
hide factory dep add|remove <task> --on <task>
hide factory pause|resume|retry|merge|cancel|revive <task>
hide factory request-changes <task> --comment <text>
hide factory check --at intake|after-done|periodic --instruction <text> [--project <path>]
hide factory close [--project <path>]
Add --json to print the answer as JSON.";

/// Prints an answer for a person. Every answer carries `ok`; a refusal
/// prints its reason and next step.
pub fn render_human(verb: &str, answer: &Value) -> String {
    if answer["ok"] != true {
        let mut text = format!(
            "refused: {}",
            answer["reason"].as_str().unwrap_or("unavailable")
        );
        if let Some(detail) = answer.get("detail") {
            if let Some(state) = detail["state"].as_str() {
                text.push_str(&format!("\ncurrent state: {state}"));
            }
            if let Some(actions) = detail["allowed"].as_array() {
                let names: Vec<_> = actions.iter().filter_map(Value::as_str).collect();
                text.push_str(&format!("\nallowed: {}", names.join(", ")));
            }
            if let Some(fields) = detail["fields"].as_array() {
                for field in fields {
                    text.push_str(&format!(
                        "\n- {}: {}",
                        field["field"].as_str().unwrap_or("?"),
                        field["problem"].as_str().unwrap_or("?")
                    ));
                }
            }
        }
        if let Some(next) = answer["next_action"].as_str() {
            text.push_str(&format!("\nnext: {next}"));
        }
        return text;
    }
    match verb {
        "status" => render_status(answer),
        "show" => render_show(&answer["task"]),
        "inbox" => render_inbox(answer),
        "init" => render_init(answer),
        "add" => {
            let mut text = format!(
                "{} {}: {}",
                answer["task"]["display_id"].as_str().unwrap_or("?"),
                answer["result"].as_str().unwrap_or("?"),
                answer["task"]["title"].as_str().unwrap_or("")
            );
            if let Some(questions) = answer["questions"].as_array() {
                for question in questions {
                    text.push_str(&format!(
                        "\n? {} (suggestion: {})",
                        question["text"].as_str().unwrap_or(""),
                        question["suggestion"].as_str().unwrap_or("")
                    ));
                }
            }
            text
        }
        "config" => serde_json::to_string_pretty(
            &serde_json::json!({"config": answer["config"], "machine": answer["machine"]}),
        )
        .unwrap_or_default(),
        _ => {
            let mut text = answer["message"].as_str().unwrap_or("ok").to_owned();
            if let Some(task) = answer.get("task")
                && let (Some(id), Some(state)) =
                    (task["display_id"].as_str(), task["state"].as_str())
            {
                text.push_str(&format!("\n{id}: {state}"));
            }
            text
        }
    }
}

fn render_init(answer: &Value) -> String {
    let mut text = String::new();
    if answer["created"] == true {
        text.push_str(&format!(
            "Factory {} created for {}\n",
            answer["factory"]["id"].as_str().unwrap_or("?"),
            answer["factory"]["project"].as_str().unwrap_or("?")
        ));
        text.push_str("Add a Task: hide factory add --title <t> --goal <g> --criterion <c>");
        return text;
    }
    if answer["existing"] == true {
        return format!(
            "This project already has Factory {}",
            answer["factory"]["id"].as_str().unwrap_or("?")
        );
    }
    text.push_str(&format!(
        "project: {}\nsource: {}\n",
        answer["project"].as_str().unwrap_or("?"),
        answer["source"].as_str().unwrap_or("?")
    ));
    text.push_str("verification candidates:\n");
    for candidate in answer["candidates"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "  - {} {}\n",
            candidate["kind"].as_str().unwrap_or("?"),
            candidate["value"].as_str().unwrap_or("")
        ));
    }
    text.push_str(&format!(
        "merge mode: {}\n",
        answer["merge_mode"].as_str().unwrap_or("?")
    ));
    if let Some(note) = answer["auto_unavailable"].as_str() {
        text.push_str(&format!("auto: unavailable ({note})\n"));
    }
    for write in answer["writes"].as_array().into_iter().flatten() {
        text.push_str(&format!("will write: {}\n", write.as_str().unwrap_or("")));
    }
    text.push_str("Nothing was written. Choose a verification and add --confirm.");
    text
}

fn render_status(answer: &Value) -> String {
    let mut text = String::new();
    if let Some(failures) = answer["store_failures"].as_u64().filter(|n| *n > 0) {
        text.push_str(&format!(
            "store writes failed {failures} times since start; see the diagnostic log\n"
        ));
    }
    for factory in answer["factories"].as_array().into_iter().flatten() {
        let flow = &factory["flow"];
        text.push_str(&format!(
            "{} {}  정리 중 {} · 대기 {} · 실행 중 {} · 완료 오늘 {}  내 차례 {}\n",
            factory["id"].as_str().unwrap_or("?"),
            factory["project_name"].as_str().unwrap_or("?"),
            flow["drafting"],
            flow["waiting"],
            flow["running"],
            flow["done_today"],
            factory["my_turn"],
        ));
        if let Some(read) = factory["outside_read_at"].as_u64() {
            text.push_str(&format!("  outside read at {read}"));
            if factory["stale"] == true {
                text.push_str(" (stale)");
            }
            text.push('\n');
        }
        if factory["main_broken"] == true {
            text.push_str("  main broken: auto merge stopped\n");
        }
        for column in factory["columns"].as_array().into_iter().flatten() {
            text.push_str(&format!(
                "  [{}]\n",
                column["label"].as_str().unwrap_or("?")
            ));
            for card in column["cards"].as_array().into_iter().flatten() {
                let mark = if card["needs_person"] == true {
                    "!"
                } else {
                    " "
                };
                text.push_str(&format!(
                    "   {mark} {:<8} {:<10} {}{}\n",
                    card["display_id"].as_str().unwrap_or("?"),
                    card["state_label"].as_str().unwrap_or("?"),
                    card["title"].as_str().unwrap_or(""),
                    card["waiting_for"]
                        .as_str()
                        .map(|wait| format!("  (waits: {wait})"))
                        .unwrap_or_default()
                ));
            }
        }
    }
    if text.is_empty() {
        text.push_str("No Factory. Create one: hide factory init <project>");
    }
    text
}

fn render_show(task: &Value) -> String {
    let card = &task["card"];
    let mut text = format!(
        "{} {} [{}]\n",
        card["display_id"].as_str().unwrap_or("?"),
        card["title"].as_str().unwrap_or(""),
        card["state_label"].as_str().unwrap_or("?")
    );
    text.push_str(&format!("goal: {}\n", task["goal"].as_str().unwrap_or("")));
    for criterion in task["criteria"].as_array().into_iter().flatten() {
        text.push_str(&format!("  ✓ {}\n", criterion.as_str().unwrap_or("")));
    }
    for item in task["out_of_scope"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "  out of scope: {}\n",
            item.as_str().unwrap_or("")
        ));
    }
    let chain = |key: &str| {
        task[key]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };
    text.push_str(&format!(
        "chain: [{}] -> {} -> [{}]\n",
        chain("before"),
        card["display_id"].as_str().unwrap_or("?"),
        chain("after")
    ));
    if let Some(external) = card["external"]
        .as_array()
        .filter(|items| !items.is_empty())
    {
        let names: Vec<_> = external.iter().filter_map(Value::as_str).collect();
        text.push_str(&format!("외부 대기: {}\n", names.join(", ")));
    }
    text.push_str(&format!(
        "verification {}",
        task["verification"].as_str().unwrap_or("?")
    ));
    if let Some(pr) = task["pr"]["url"].as_str() {
        text.push_str(&format!("  PR {pr}"));
    }
    if let Some(pane) = card["worker_pane"].as_str() {
        text.push_str(&format!("  worker {pane}"));
    }
    text.push('\n');
    for attempt in task["attempts"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "attempt {} {} {}{}\n",
            attempt["number"],
            attempt["stage"].as_str().unwrap_or("?"),
            attempt["outcome"].as_str().unwrap_or("?"),
            attempt["check"]
                .as_str()
                .map(|check| format!(": {check}"))
                .unwrap_or_default()
        ));
    }
    if let Some(gates) = task["gates"].as_array().filter(|g| !g.is_empty()) {
        let names: Vec<_> = gates.iter().filter_map(Value::as_str).collect();
        text.push_str(&format!("merge waits for: {}\n", names.join(", ")));
    }
    for attachment in task["attachments"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "attachment v{} {} sha256:{}\n",
            attachment["version"],
            attachment["path"].as_str().unwrap_or(""),
            attachment["sha256"].as_str().unwrap_or("")
        ));
    }
    for decision in task["decisions"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "decision ({}): {}\n",
            decision["by"].as_str().unwrap_or("?"),
            decision["text"].as_str().unwrap_or("")
        ));
    }
    for question in task["questions"].as_array().into_iter().flatten() {
        let state = if question["answer"].is_null() {
            "open".to_owned()
        } else {
            format!(
                "answered by {}",
                question["answer"]["relayed_by"].as_str().unwrap_or("?")
            )
        };
        text.push_str(&format!(
            "question {} [{state}]: {}\n",
            question["id"].as_str().unwrap_or("?"),
            question["text"].as_str().unwrap_or("")
        ));
    }
    if let Some(allowed) = task["allowed"].as_array() {
        let names: Vec<_> = allowed.iter().filter_map(Value::as_str).collect();
        text.push_str(&format!("allowed: {}\n", names.join(", ")));
    }
    text
}

fn render_inbox(answer: &Value) -> String {
    let mut text = format!("내 차례 {}\n", answer["count"]);
    for item in answer["items"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "- [{}] {} {} ({}) {}\n",
            item["group"].as_str().unwrap_or("?"),
            item["display_id"].as_str().unwrap_or("?"),
            item["title"].as_str().unwrap_or(""),
            item["project"].as_str().unwrap_or(""),
            item["text"].as_str().unwrap_or("")
        ));
        if let Some(suggestion) = item["suggestion"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            text.push_str(&format!("    suggestion: {suggestion}\n"));
        }
        if let Some(default) = item["default_action"].as_str() {
            text.push_str(&format!("    default: {default}\n"));
        }
        if let Some(remaining) = item["remaining"].as_str() {
            text.push_str(&format!("    {remaining}\n"));
        }
        if let Some(question) = item["question"].as_str() {
            text.push_str(&format!(
                "    answer: hide factory answer {} --question {question} --choose suggestion\n",
                item["display_id"].as_str().unwrap_or("?")
            ));
        }
    }
    text
}
