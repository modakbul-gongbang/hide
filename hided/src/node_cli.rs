//! `hided node`: this program in its node role on a registered device,
//! started by the core over the stdin and stdout of an SSH exec channel
//! (PRD core-host-node D-02). `serve` answers the node contract until the
//! channel closes; it opens no socket. `workspace-bridge` and `pane-inspect`
//! are the device's pane-command bridge and its proof read, which layer 2b
//! replaces with proofs sent up the link.

use std::ffi::OsString;
use std::io::{self, BufReader};

/// What a device answers to `hided node --version`: the role, the build and
/// the node protocol it speaks.
pub fn version_line() -> String {
    format!(
        "hided-node {} protocol {}",
        env!("CARGO_PKG_VERSION"),
        hide_host::protocol::PROTOCOL_VERSION
    )
}

const USAGE: &str =
    "usage: hided node serve|workspace-bridge|pane-inspect <socket> <pane-id>|--version";

/// Runs the node subcommand named by `args`, the words after `node`.
pub fn run(args: &[OsString]) -> Result<(), String> {
    let words: Vec<&str> = args
        .iter()
        .map(|arg| arg.to_str().ok_or_else(|| USAGE.to_owned()))
        .collect::<Result<_, _>>()?;
    match words.as_slice() {
        ["serve"] => {
            let input = BufReader::new(io::stdin().lock());
            hide_host::serve::serve(input, io::stdout()).map_err(|error| error.to_string())
        }
        ["workspace-bridge"] => {
            let input = BufReader::new(io::stdin());
            hide_host::workspace_bridge::serve(input, io::stdout())
                .map_err(|error| error.to_string())
        }
        ["pane-inspect", socket, pane_id] => {
            let identity = hide_host::pane_peer::inspect(std::path::Path::new(socket), pane_id)?;
            println!(
                "{}",
                serde_json::to_string(&identity).map_err(|error| error.to_string())?
            );
            Ok(())
        }
        ["--version"] => {
            println!("{}", version_line());
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    }
}
