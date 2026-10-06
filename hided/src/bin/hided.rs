fn main() {
    // Taken before anything else, as every cooperative child takes it: a
    // starter that passes an owner channel (the web e2e fixture) ends this
    // daemon and what it started by ending itself, even when it is killed.
    // `hide connect` starts the daemon without one, through `spawn_owned`,
    // because the daemon outlives the app that started it.
    let owner_watch = match hide_platform::process::OwnerWatch::from_launch() {
        Ok(watch) => watch,
        Err(error) => {
            eprintln!("owner watch acquisition failed: {error}");
            std::process::exit(2);
        }
    };
    let result = run();
    // After `run` has dropped the runtime, and with it the core.
    if let Some(watch) = owner_watch
        && let Err(error) = watch.close()
    {
        eprintln!("owner watch cleanup failed: {error}");
        std::process::exit(2);
    }
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "--open-helper") {
        // Only Unix supervises a file opener through this mode.
        #[cfg(unix)]
        return hided::spawn::run_opener_helper(&args);
        #[cfg(not(unix))]
        return Err("--open-helper is the Unix opener supervisor's mode".into());
    }
    let mut env = hided::env::load().map_err(|errors| {
        errors
            .iter()
            .map(|error| format!("{}: {}", error.key, error.kind))
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    // Hashed before anything else: an app update replaces the file under the
    // same path, and a later read would report the new build as this one.
    env.build = Some(hided::build_id::of_current_exe()?);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("hided")
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(hided::run_daemon(env))
}
