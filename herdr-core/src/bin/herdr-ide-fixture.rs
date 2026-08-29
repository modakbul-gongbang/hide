use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(name) = std::env::args().nth(1) else {
        eprintln!("usage: herdr-ide-fixture herdr-ide-verify-<name>");
        return ExitCode::from(2);
    };
    match herdr_core::fixture::plan(&name) {
        Ok(plan) => match serde_json::to_string_pretty(&plan) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(_) => {
                eprintln!("fixture plan could not be encoded");
                ExitCode::from(1)
            }
        },
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
