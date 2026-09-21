fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let env = hided::env::load().map_err(|errors| {
        errors
            .iter()
            .map(|error| format!("{}: {}", error.key, error.kind))
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("hided")
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(hided::run_daemon(env))
}
