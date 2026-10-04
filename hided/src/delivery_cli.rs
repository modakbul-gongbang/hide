//! Small typed command surface for the core-owned durable mailbox.

use herdr_core::delivery::{BODY_LIMIT, Command, HOOK_LETTERS};

use crate::env::{self, Env};

pub const USAGE: &str = "hide request send <target> --intent <key> --body <text>\nhide request reply <id> --intent <key> --body <text>\nhide request ack|cancel|show <id>\nhide inbox\nhide watch start <target>\nhide watch stop <id>\nhide watch list\nDelivery commands require a running daemon and a current agent pane; they do not require an open renderer.";

pub fn parse<'a>(
    topic: &str,
    mut args: impl Iterator<Item = &'a String>,
) -> Result<Command, String> {
    let verb = args.next().map(String::as_str);
    if topic == "inbox" {
        return match verb {
            None => Ok(Command::Inbox),
            Some("--hook") if args.next().is_none() => Ok(Command::Pull),
            Some("--confirm") => {
                let ids: Vec<_> = args.cloned().collect();
                if ids.is_empty() || ids.len() > HOOK_LETTERS || !ids.iter().all(|id| key(id)) {
                    return Err(USAGE.into());
                }
                Ok(Command::Confirm { ids })
            }
            _ => Err(USAGE.into()),
        };
    }
    if topic == "watch" && verb == Some("list") && args.next().is_none() {
        return Ok(Command::WatchList);
    }
    let subject = args.next().filter(|value| key(value)).ok_or(USAGE)?.clone();
    if topic == "watch" {
        if args.next().is_some() {
            return Err(USAGE.into());
        }
        return match verb {
            Some("start") => Ok(Command::WatchStart { target: subject }),
            Some("stop") => Ok(Command::WatchStop { id: subject }),
            _ => Err(USAGE.into()),
        };
    }
    match verb {
        Some("send" | "reply") => {
            let mut intent = None;
            let mut body = None;
            while let Some(flag) = args.next() {
                let value = args.next().ok_or(USAGE)?.clone();
                match flag.as_str() {
                    "--intent" if intent.is_none() && key(&value) => intent = Some(value),
                    "--body" if body.is_none() => body = Some(value),
                    _ => return Err(USAGE.into()),
                }
            }
            let intent = intent.ok_or(USAGE)?;
            let body = body
                .filter(|body: &String| !body.trim().is_empty())
                .ok_or(USAGE)?;
            if body.len() > BODY_LIMIT {
                return Err("capacity".into());
            }
            if verb == Some("send") {
                Ok(Command::Send {
                    target: subject,
                    intent,
                    body,
                })
            } else {
                Ok(Command::Reply {
                    id: subject,
                    intent,
                    body,
                })
            }
        }
        Some("ack" | "cancel" | "show") if args.next().is_none() => match verb {
            Some("ack") => Ok(Command::Ack { id: subject }),
            Some("cancel") => Ok(Command::Cancel { id: subject }),
            _ => Ok(Command::Show { id: subject }),
        },
        _ => Err(USAGE.into()),
    }
}

fn key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.starts_with('-')
        && !value.chars().any(char::is_control)
}

pub fn run(env: &Env, command: Command) -> Result<(), String> {
    let result = (|| {
        let (reference, ephemeral) = crate::cli::workspace_reference(env)?;
        let _owner = ephemeral.then(|| crate::workspace_cli::OneShotReference(reference.clone()));
        let hint = std::env::var(env::HERDR_PANE_ID).ok();
        crate::workspace_cli::request_delivery(&reference, command, hint.as_deref())
    })();
    match result {
        Ok(answer) => {
            println!("{answer}");
            if answer["ok"] == true {
                Ok(())
            } else {
                Err(answer["reason"]
                    .as_str()
                    .unwrap_or("delivery_unavailable")
                    .to_owned())
            }
        }
        Err(code) => {
            println!(
                "{}",
                serde_json::json!({"ok":false,"reason":code,"next_action":"Check the running daemon and current agent pane, then retry the same intent"})
            );
            Err(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse_line(line: &[&str]) -> Result<Command, String> {
        let args: Vec<_> = line.iter().map(|value| (*value).to_owned()).collect();
        parse(&args[0], args.iter().skip(1))
    }
    #[test]
    fn typed_cli_requires_intent_and_has_no_wait_or_dispatch_surface() {
        assert_eq!(parse_line(&["inbox"]).unwrap(), Command::Inbox);
        assert!(parse_line(&["inbox", "wait"]).is_err());
        assert!(parse_line(&["request", "send", "target", "--body", "body"]).is_err());
        assert_eq!(
            parse_line(&[
                "request", "send", "target", "--intent", "key", "--body", "body"
            ])
            .unwrap(),
            Command::Send {
                target: "target".into(),
                intent: "key".into(),
                body: "body".into()
            }
        );
        assert!(parse_line(&["watch", "start", "target", "--force"]).is_err());
        assert!(parse_line(&["inbox", "--confirm", "a", "b", "c", "d", "e", "f"]).is_err());
    }
}
