//! Participant registration and child creation use the daemon's same durable
//! store and pane capability as the mailbox.
use crate::env::{self, Env};
use herdr_core::{coordination::Command, delivery::Command as Delivery};
use std::collections::BTreeMap;
pub const USAGE: &str = "hide agent register [--check] [--machine <device>] --host-scope <scope> --session <session> --instance <terminal> --name <name> --pane <pane> [--parent <id>] [--project <path>]\nhide agent list\nhide agent show <id>\nhide agent end <id> [--actor <id>]\nhide agent spawn --parent <here|id> --name <name> --intent <key> --kind <kind> --repo <path> --branch <branch> [--path <path>] [--no-watch] [-- <native args>]";
pub fn parse<'a>(mut args: impl Iterator<Item = &'a String>) -> Result<Delivery, String> {
    let verb = args.next().ok_or(USAGE)?.as_str();
    let mut flags = BTreeMap::new();
    let mut check = false;
    let mut no_watch = false;
    let mut id = None;
    let mut native = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => {}
            "--check" if verb == "register" && !check => check = true,
            "--no-watch" if verb == "spawn" && !no_watch => no_watch = true,
            "--" if verb == "spawn" => {
                native.extend(args.cloned());
                break;
            }
            flag if flag.starts_with("--") => {
                let allowed = match verb {
                    "register" => [
                        "--machine",
                        "--host-scope",
                        "--session",
                        "--instance",
                        "--name",
                        "--pane",
                        "--parent",
                        "--project",
                    ]
                    .as_slice(),
                    "spawn" => [
                        "--parent", "--name", "--intent", "--kind", "--repo", "--branch", "--path",
                    ]
                    .as_slice(),
                    "end" => ["--actor"].as_slice(),
                    _ => &[],
                };
                if !allowed.contains(&flag) || flags.contains_key(flag) {
                    return Err(format!("Unknown or repeated flag: {flag}"));
                }
                let value = args
                    .next()
                    .filter(|value| {
                        !value.is_empty()
                            && !value.starts_with("--")
                            && !value.chars().any(char::is_control)
                    })
                    .ok_or(USAGE)?
                    .clone();
                flags.insert(flag.to_owned(), value);
            }
            _ if matches!(verb, "show" | "end") && id.is_none() => id = Some(arg.clone()),
            _ => return Err(USAGE.into()),
        }
    }
    let mut take = |name: &str| {
        flags
            .remove(name)
            .ok_or_else(|| format!("Required flag: {name}"))
    };
    let command = match verb {
        "register" => {
            let host_scope = take("--host-scope")?;
            let session = take("--session")?;
            let instance = take("--instance")?;
            let name = take("--name")?;
            let pane = take("--pane")?;
            Command::Register {
                check,
                machine: flags.remove("--machine"),
                host_scope,
                session,
                instance,
                name,
                pane,
                parent: flags.remove("--parent"),
                project: flags.remove("--project"),
            }
        }
        "spawn" => {
            let parent = take("--parent")?;
            let name = take("--name")?;
            let intent = take("--intent")?;
            let kind = take("--kind")?;
            let repo = take("--repo")?;
            let branch = take("--branch")?;
            Command::Spawn {
                parent,
                name,
                intent,
                kind,
                repo,
                branch,
                path: flags.remove("--path"),
                no_watch,
                args: native,
            }
        }
        "list" => Command::List,
        "show" => Command::Show {
            id: id.ok_or(USAGE)?,
        },
        "end" => Command::End {
            id: id.ok_or(USAGE)?,
            actor: flags.remove("--actor"),
        },
        _ => return Err(USAGE.into()),
    };
    Ok(Delivery::Agents { command })
}
pub fn run(env: &Env, command: Delivery) -> Result<(), String> {
    let result = (|| {
        let mut credential = crate::workspace_cli::Credential::acquire(env)?;
        let hint = std::env::var(env::HERDR_PANE_ID).ok();
        crate::workspace_cli::request_delivery(&mut credential, command, hint.as_deref())
    })();
    match result {
        Ok(answer) if answer["ok"] == true => {
            println!(
                "{}",
                serde_json::json!({"ok":true,"value":answer["result"]})
            );
            Ok(())
        }
        Ok(answer) => {
            let code = answer["reason"]
                .as_str()
                .unwrap_or("agent_unavailable")
                .to_owned();
            // The daemon's next step is what the caller can act on.
            let message = answer["next_action"].as_str().unwrap_or(&code);
            println!(
                "{}",
                serde_json::json!({"ok":false,"error":{"code":code,"message":message}})
            );
            Err(code)
        }
        Err(code) => {
            println!(
                "{}",
                serde_json::json!({"ok":false,"error":{"code":code,"message":code}})
            );
            Err(code)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn line(args: &[&str]) -> Result<Delivery, String> {
        let args = args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
        parse(args.iter())
    }
    #[test]
    fn spawn_preserves_native_arguments_and_refuses_retired_flags() {
        let args = [
            "spawn",
            "--parent",
            "here",
            "--name",
            "worker",
            "--intent",
            "one",
            "--kind",
            "codex",
            "--repo",
            "/fixture",
            "--branch",
            "topic",
            "--no-watch",
            "--",
            "--model",
            "fixture model",
        ];
        let Delivery::Agents {
            command: Command::Spawn { args, no_watch, .. },
        } = line(&args).unwrap()
        else {
            panic!("spawn")
        };
        assert!(no_watch);
        assert_eq!(args, ["--model", "fixture model"]);
        for flag in [
            "--reconcile-pane",
            "--resume-start",
            "--session",
            "--interval",
        ] {
            assert!(line(&["spawn", flag, "value"]).is_err());
        }
    }
    #[test]
    fn register_surface_matches_external_caller() {
        let register = |machine: &[&str]| {
            let mut args = vec!["register", "--check"];
            args.extend_from_slice(machine);
            args.extend_from_slice(&[
                "--host-scope",
                "fixture",
                "--session",
                "session",
                "--instance",
                "terminal",
                "--name",
                "worker",
                "--pane",
                "w1:p1",
                "--json",
            ]);
            match line(&args) {
                Ok(Delivery::Agents {
                    command: Command::Register { machine, .. },
                }) => machine,
                other => panic!("register: {other:?}"),
            }
        };
        assert_eq!(register(&[]), None);
        assert_eq!(register(&["--machine", "mini"]).as_deref(), Some("mini"));
        assert!(line(&["list", "unexpected"]).is_err());
        assert!(line(&["end", "agent-1", "--actor", "agent-2"]).is_ok());
    }
}
