//! Serves the file host contract on a registered device, over the stdin and
//! stdout of the SSH exec channel the core opened. It takes no arguments but
//! `serve`, opens no socket, and exits when the channel closes.

use std::io::{self, BufReader};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    match (arguments.next().as_deref(), arguments.next()) {
        (Some("serve"), None) => {
            let input = BufReader::new(io::stdin().lock());
            match hide_host::serve::serve(input, io::stdout()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("hide-host-helper: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        // The pane bootstrap commands stand on a Unix socket (see
        // `hide_host::workspace_bridge`).
        #[cfg(unix)]
        (Some("workspace-bridge"), None) => {
            let input = BufReader::new(io::stdin());
            match hide_host::workspace_bridge::serve(input, io::stdout()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("hide-host-helper: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        #[cfg(unix)]
        (Some("pane-inspect"), Some(socket)) => {
            let Some(pane_id) = arguments.next() else {
                eprintln!("usage: hide-host-helper pane-inspect <socket> <pane-id>");
                return ExitCode::from(2);
            };
            if arguments.next().is_some() {
                return ExitCode::from(2);
            }
            match hide_host::workspace_bridge::inspect(std::path::Path::new(&socket), &pane_id) {
                Ok(identity) => {
                    println!(
                        "{}",
                        serde_json::to_string(&identity).expect("pane identity JSON")
                    );
                    ExitCode::SUCCESS
                }
                Err(reason) => {
                    eprintln!("hide-host-helper: {reason}");
                    ExitCode::FAILURE
                }
            }
        }
        (Some("--version"), None) => {
            println!(
                "hide-host-helper {} protocol {}",
                env!("CARGO_PKG_VERSION"),
                hide_host::protocol::PROTOCOL_VERSION
            );
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "usage: hide-host-helper serve|workspace-bridge|pane-inspect <socket> <pane-id>"
            );
            ExitCode::from(2)
        }
    }
}
