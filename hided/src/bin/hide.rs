fn main() {
    let args: Vec<String> = std::env::args().collect();
    let parsed = hided::cli::parse_args(&args);
    if let Err(error) = &parsed
        && matches!(
            args.get(1).map(String::as_str),
            Some("workspace" | "file" | "diff" | "view")
        )
    {
        println!(
            "{}",
            serde_json::json!({
                "ok": false,
                "reason": "invalid_arguments",
                "next_action": error,
            })
        );
        std::process::exit(2);
    }
    match parsed.and_then(hided::cli::run) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
