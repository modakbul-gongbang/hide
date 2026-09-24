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
        (Some("--version"), None) => {
            println!(
                "hide-host-helper {} protocol {}",
                env!("CARGO_PKG_VERSION"),
                hide_host::protocol::PROTOCOL_VERSION
            );
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: hide-host-helper serve");
            ExitCode::from(2)
        }
    }
}
