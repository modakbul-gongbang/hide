//! `hide factory ...`: parses each subcommand into the typed Factory command
//! and prints the engine's answer for a person, or as JSON with `--json`.
//! Which caller may run which command is the engine's decision (D-41).

use std::path::Path;

use hide_factory::Command;
use hide_factory::command::{CardInput, USAGE, VerificationChoice, render_human};
use hide_factory::model::{CheckPoint, DiscoveryClass, MergeMode, Runtime};

use crate::env::{self, Env};

#[derive(Debug, Eq, PartialEq)]
pub struct FactoryRequest {
    pub command: Command,
    pub json: bool,
}

pub fn parse<'a>(args: impl Iterator<Item = &'a String>) -> Result<FactoryRequest, String> {
    let mut json = false;
    let mut words = Vec::new();
    for arg in args {
        if arg == "--json" {
            json = true;
        } else {
            words.push(arg.as_str());
        }
    }
    let cwd = std::env::current_dir().map_err(|_| "current_directory_unavailable".to_owned())?;
    let command = parse_words(&words, &cwd).ok_or_else(|| USAGE.to_owned())?;
    Ok(FactoryRequest { command, json })
}

/// The flags of one subcommand, read in order. A flag given twice where it
/// takes one value, an unknown flag, or a missing value is a usage error.
struct Flags<'a> {
    rest: std::iter::Peekable<std::slice::Iter<'a, &'a str>>,
}

impl<'a> Flags<'a> {
    fn new(words: &'a [&'a str]) -> Self {
        Self {
            rest: words.iter().peekable(),
        }
    }

    fn next(&mut self) -> Option<&'a str> {
        self.rest.next().copied()
    }

    fn value(&mut self) -> Option<String> {
        self.rest
            .next()
            .filter(|value| !value.starts_with("--"))
            .map(|value| (*value).to_owned())
    }

    /// Values up to the next flag (`--ci check-a check-b`).
    fn values(&mut self) -> Vec<String> {
        let mut values = Vec::new();
        while let Some(value) = self.rest.peek() {
            if value.starts_with("--") {
                break;
            }
            values.push((**value).to_owned());
            self.rest.next();
        }
        values
    }
}

fn once(slot: &mut Option<String>, value: Option<String>) -> Option<()> {
    if slot.is_some() {
        return None;
    }
    *slot = Some(value?);
    Some(())
}

/// A path the daemon reads: absolute, because the daemon does not share the
/// caller's working directory.
fn absolute(cwd: &Path, value: &str) -> String {
    let path = cwd.join(value);
    std::fs::canonicalize(&path)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn merge_mode(value: &str) -> Option<MergeMode> {
    match value {
        "auto" => Some(MergeMode::Auto),
        "manual" => Some(MergeMode::Manual),
        _ => None,
    }
}

fn parse_words(words: &[&str], cwd: &Path) -> Option<Command> {
    let (verb, rest) = words.split_first()?;
    match *verb {
        "init" => parse_init(rest, cwd),
        "add" => parse_add(rest, cwd),
        "status" | "close" => {
            let project = project_only(rest, cwd)?;
            Some(if *verb == "status" {
                Command::Status { project }
            } else {
                Command::Close { project }
            })
        }
        "show" => match rest {
            [task] if !task.starts_with("--") => Some(Command::Show {
                task: (*task).to_owned(),
            }),
            _ => None,
        },
        "inbox" => rest.is_empty().then_some(Command::Inbox),
        "answer" => parse_answer(rest),
        "ask" | "block" => parse_question(verb, rest),
        "propose" => parse_propose(rest, cwd),
        "done" => {
            let mut flags = Flags::new(rest);
            let mut summary = None;
            let mut breaking = false;
            while let Some(flag) = flags.next() {
                match flag {
                    "--summary" => once(&mut summary, flags.value())?,
                    "--breaking" if !breaking => breaking = true,
                    _ => return None,
                }
            }
            Some(Command::Done {
                summary,
                breaking,
                letter: None,
            })
        }
        "decide" => match rest {
            ["--text", text] => Some(Command::Decide {
                text: (*text).to_owned(),
            }),
            _ => None,
        },
        "config" => {
            let mut flags = Flags::new(rest);
            let mut project = None;
            let mut set = Vec::new();
            while let Some(flag) = flags.next() {
                match flag {
                    "--project" => once(&mut project, flags.value().map(|p| absolute(cwd, &p)))?,
                    "--set" => {
                        let pair = flags.value()?;
                        let (key, value) = pair.split_once('=')?;
                        if key.is_empty() {
                            return None;
                        }
                        set.push((key.to_owned(), value.to_owned()));
                    }
                    _ => return None,
                }
            }
            Some(Command::Config { project, set })
        }
        "priority" => match rest {
            [task, priority] => Some(Command::Priority {
                task: (*task).to_owned(),
                priority: priority.parse().ok()?,
            }),
            _ => None,
        },
        "dep" => match rest {
            [action @ ("add" | "remove"), task, "--on", on] if !task.starts_with("--") => {
                Some(Command::Dep {
                    task: (*task).to_owned(),
                    on: (*on).to_owned(),
                    remove: *action == "remove",
                })
            }
            _ => None,
        },
        "pause" | "resume" if rest.first() == Some(&"--factory") => {
            let project = project_only(&rest[1..], cwd)?;
            Some(if *verb == "pause" {
                Command::PauseFactory { project }
            } else {
                Command::ResumeFactory { project }
            })
        }
        "ack-notices" => Some(Command::AckNotices {
            project: project_only(rest, cwd)?,
        }),
        "worker" => match rest {
            [task, choice] if !task.starts_with("--") => Some(Command::Worker {
                task: (*task).to_owned(),
                worker: match *choice {
                    "auto" => None,
                    number => Some(number.parse().ok().filter(|n| *n > 0)?),
                },
            }),
            _ => None,
        },
        "pause" | "resume" | "retry" | "merge" | "cancel" | "revive" => {
            let [task] = rest else { return None };
            if task.starts_with("--") {
                return None;
            }
            let task = (*task).to_owned();
            Some(match *verb {
                "pause" => Command::Pause { task },
                "resume" => Command::Resume { task },
                "retry" => Command::Retry { task },
                "merge" => Command::Merge { task },
                "cancel" => Command::Cancel { task },
                _ => Command::Revive { task },
            })
        }
        "request-changes" => match rest {
            [task, "--comment", comment] if !task.starts_with("--") => {
                Some(Command::RequestChanges {
                    task: (*task).to_owned(),
                    comment: (*comment).to_owned(),
                })
            }
            _ => None,
        },
        "check" => {
            let mut flags = Flags::new(rest);
            let mut project = None;
            let mut at = None;
            let mut instruction = None;
            while let Some(flag) = flags.next() {
                match flag {
                    "--project" => once(&mut project, flags.value().map(|p| absolute(cwd, &p)))?,
                    "--at" => once(&mut at, flags.value())?,
                    "--instruction" => once(&mut instruction, flags.value())?,
                    _ => return None,
                }
            }
            let at = match at?.as_str() {
                "intake" => CheckPoint::Intake,
                "after-done" | "after_done" => CheckPoint::AfterDone,
                "periodic" => CheckPoint::Periodic,
                _ => return None,
            };
            Some(Command::Check {
                project,
                at,
                instruction: instruction?,
            })
        }
        _ => None,
    }
}

fn project_only(rest: &[&str], cwd: &Path) -> Option<Option<String>> {
    match rest {
        [] => Some(None),
        ["--project", path] if !path.starts_with("--") => Some(Some(absolute(cwd, path))),
        _ => None,
    }
}

fn parse_init(rest: &[&str], cwd: &Path) -> Option<Command> {
    let (project, rest) = rest.split_first()?;
    if project.starts_with("--") {
        return None;
    }
    let mut flags = Flags::new(rest);
    let mut ci: Option<Vec<String>> = None;
    let mut commands = Vec::new();
    let mut none = false;
    let mut merge = None;
    let mut confirm = false;
    while let Some(flag) = flags.next() {
        match flag {
            "--ci" if ci.is_none() => ci = Some(flags.values()),
            "--verify" => commands.push(flags.value()?),
            "--no-verification" if !none => none = true,
            "--merge" => once(&mut merge, flags.value())?,
            "--confirm" if !confirm => confirm = true,
            _ => return None,
        }
    }
    let verification = match (ci, commands.is_empty(), none) {
        (None, true, false) => None,
        (Some(checks), true, false) => Some(VerificationChoice::Ci { checks }),
        (None, false, false) => Some(VerificationChoice::Commands { commands }),
        (None, true, true) => Some(VerificationChoice::None),
        _ => return None,
    };
    Some(Command::Init {
        project: absolute(cwd, project),
        verification,
        merge_mode: match merge {
            Some(value) => Some(merge_mode(&value)?),
            None => None,
        },
        confirm,
    })
}

/// The card flags shared by `add` and `propose`.
fn card_flag(flag: &str, flags: &mut Flags<'_>, card: &mut CardInput, cwd: &Path) -> Option<()> {
    match flag {
        "--title" => once(&mut card.title, flags.value()),
        "--goal" => once(&mut card.goal, flags.value()),
        "--criterion" => {
            card.criteria.push(flags.value()?);
            Some(())
        }
        "--out-of-scope" => {
            card.out_of_scope.push(flags.value()?);
            Some(())
        }
        "--open" => {
            card.open_decisions.push(flags.value()?);
            Some(())
        }
        "--after" => {
            card.depends_on.push(flags.value()?);
            Some(())
        }
        "--external" => {
            card.external.push(flags.value()?);
            Some(())
        }
        "--prd" => once(&mut card.prd, flags.value().map(|p| absolute(cwd, &p))),
        "--review-directly" if !card.review_directly => {
            card.review_directly = true;
            Some(())
        }
        "--priority" if card.priority.is_none() => {
            card.priority = Some(flags.value()?.parse().ok()?);
            Some(())
        }
        "--merge" if card.merge_mode.is_none() => {
            card.merge_mode = Some(merge_mode(&flags.value()?)?);
            Some(())
        }
        "--runtime" if card.runtime.is_none() => {
            card.runtime = Some(Runtime::parse(&flags.value()?)?);
            Some(())
        }
        "--worker" if card.worker.is_none() => {
            card.worker = Some(flags.value()?.parse().ok().filter(|n| *n > 0)?);
            Some(())
        }
        _ => None,
    }
}

fn parse_add(rest: &[&str], cwd: &Path) -> Option<Command> {
    let (issue, rest) = match rest.split_first() {
        Some((first, rest)) if !first.starts_with("--") => (Some((*first).to_owned()), rest),
        _ => (None, rest),
    };
    let mut flags = Flags::new(rest);
    let mut card = CardInput::default();
    let mut project = None;
    let mut task = None;
    while let Some(flag) = flags.next() {
        match flag {
            "--project" => once(&mut project, flags.value().map(|p| absolute(cwd, &p)))?,
            "--task" => once(&mut task, flags.value())?,
            _ => card_flag(flag, &mut flags, &mut card, cwd)?,
        }
    }
    if issue.is_some() && task.is_some() {
        return None;
    }
    Some(Command::Add {
        project,
        task,
        issue,
        card,
        producer_pane: None,
    })
}

fn parse_answer(rest: &[&str]) -> Option<Command> {
    let (task, rest) = rest.split_first()?;
    if task.starts_with("--") {
        return None;
    }
    let mut flags = Flags::new(rest);
    let mut question = None;
    let mut choice = None;
    let mut text = None;
    let mut change = false;
    while let Some(flag) = flags.next() {
        match flag {
            "--question" => once(&mut question, flags.value())?,
            "--choose" => once(&mut choice, flags.value())?,
            "--text" => once(&mut text, flags.value())?,
            "--change" if !change => change = true,
            _ => return None,
        }
    }
    if choice.is_none() && text.is_none() {
        return None;
    }
    Some(Command::Answer {
        task: (*task).to_owned(),
        question,
        choice,
        text,
        change,
    })
}

fn parse_question(verb: &str, rest: &[&str]) -> Option<Command> {
    let mut flags = Flags::new(rest);
    let mut text = None;
    let mut suggestion = None;
    let mut default_action = None;
    let mut deadline = None;
    let mut choices = Vec::new();
    while let Some(flag) = flags.next() {
        match flag {
            "--question" => once(&mut text, flags.value())?,
            "--suggestion" => once(&mut suggestion, flags.value())?,
            "--default" if verb == "ask" => once(&mut default_action, flags.value())?,
            "--deadline-hours" => once(&mut deadline, flags.value())?,
            // Their count and length are the engine's to refuse (B1).
            "--choice" => choices.push(flags.value()?),
            _ => return None,
        }
    }
    let deadline_hours = match deadline {
        Some(hours) => Some(hours.parse().ok()?),
        None => None,
    };
    Some(if verb == "ask" {
        Command::Ask {
            text: text?,
            suggestion: suggestion?,
            default_action: default_action?,
            deadline_hours,
            letter: None,
            choices,
        }
    } else {
        Command::Block {
            text: text?,
            suggestion: suggestion?,
            deadline_hours,
            letter: None,
            choices,
        }
    })
}

fn parse_propose(rest: &[&str], cwd: &Path) -> Option<Command> {
    let mut flags = Flags::new(rest);
    let mut class = None;
    let mut text = None;
    let mut autonomy = None;
    let mut reclassify = None;
    let mut card = CardInput::default();
    while let Some(flag) = flags.next() {
        match flag {
            "--class" => once(&mut class, flags.value())?,
            "--text" => once(&mut text, flags.value())?,
            "--autonomy" => once(&mut autonomy, flags.value())?,
            "--reclassify" => once(&mut reclassify, flags.value())?,
            _ => card_flag(flag, &mut flags, &mut card, cwd)?,
        }
    }
    Some(Command::Propose {
        class: DiscoveryClass::parse(&class?)?,
        text: text?,
        card: (card != CardInput::default()).then_some(card),
        autonomy,
        reclassify,
        letter: None,
    })
}

pub fn run(env: &Env, request: FactoryRequest) -> Result<(), String> {
    let verb = request.command.verb();
    let answer = (|| {
        let mut credential = crate::workspace_cli::Credential::acquire(env)?;
        let hint = std::env::var(env::HERDR_PANE_ID).ok();
        crate::workspace_cli::request_factory(&mut credential, &request.command, hint.as_deref())
    })();
    // The socket answer wraps the engine's: a refusal before the engine
    // (capability, daemon) has no `result`.
    let answer = match answer {
        Ok(answer) if answer["ok"] == true => answer["result"].clone(),
        Ok(answer) => serde_json::json!({
            "ok": false,
            "reason": answer["reason"],
            "next_action": answer["next_action"],
        }),
        Err(code) => serde_json::json!({
            "ok": false,
            "reason": code,
            "next_action": "Check that Hide is running in this pane, then retry",
        }),
    };
    if request.json {
        println!("{answer}");
    } else {
        println!("{}", render_human(verb, &answer));
    }
    if answer["ok"] == true {
        Ok(())
    } else {
        Err(answer["reason"]
            .as_str()
            .unwrap_or("factory_unavailable")
            .to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_line(line: &str) -> Option<Command> {
        let words: Vec<&str> = line.split(' ').collect();
        parse_words(&words, Path::new("/work"))
    }

    #[test]
    fn every_documented_subcommand_parses() {
        for line in [
            "init /p --verify make --merge auto --confirm",
            "init /p --ci build test",
            "init /p --no-verification",
            "add --title t --goal g --criterion c --after T-1 --runtime codex --priority 2",
            "add --title t --worker 2",
            "add 12 --title t",
            "add --task T-3 --goal better",
            "status",
            "status --project /p",
            "show T-1",
            "inbox",
            "answer T-1 --choose suggestion",
            "answer T-1 --question q-1 --text yes",
            "answer T-1 --question q-1 --text no --change",
            "ask --question q --suggestion s --default d --deadline-hours 4",
            "block --question q --suggestion s",
            "block --question q --suggestion s --choice a --choice b",
            "propose --class prerequisite --text t --title a --goal b --criterion c",
            "propose --class decision --text t --reclassify d-1",
            "done --summary s --breaking",
            "decide --text t",
            "config --set merge_mode=auto --set quick_check=make",
            "priority T-1 -3",
            "dep add T-2 --on T-1",
            "dep remove T-2 --on T-1",
            "pause T-1",
            "pause --factory",
            "resume --factory --project /p",
            "worker T-1 2",
            "worker T-1 auto",
            "ack-notices",
            "revive T-1",
            "request-changes T-1 --comment c",
            "check --at after-done --instruction i",
            "close --project /p",
        ] {
            assert!(parse_line(line).is_some(), "{line}");
        }
    }

    #[test]
    fn ambiguous_or_incomplete_lines_are_refused() {
        for line in [
            "init",
            "init /p --ci a --verify b",
            "init /p --merge sometimes",
            "add 12 --task T-1",
            "add --title a --title b",
            "answer T-1",
            "ask --question q --suggestion s",
            "block --question q --suggestion s --default d",
            "propose --class nonsense --text t",
            "config --set novalue",
            "dep add T-2",
            "pause",
            "pause --factory T-1",
            "worker T-1 0",
            "worker T-1",
            "add --worker first",
            "ask --question q --suggestion s --default d --choice",
            "check --at weekly --instruction i",
            "show",
            "unknown",
        ] {
            assert_eq!(parse_line(line), None, "{line}");
        }
    }

    #[test]
    fn paths_are_made_absolute_for_the_daemon() {
        let Some(Command::Init { project, .. }) = parse_line("init repo") else {
            panic!("init parses");
        };
        assert_eq!(project, "/work/repo");
        let Some(Command::Add { card, .. }) = parse_line("add --prd docs/prd.md --title t") else {
            panic!("add parses");
        };
        assert_eq!(card.prd.as_deref(), Some("/work/docs/prd.md"));
    }
}
