fn main() {
    let args: Vec<String> = std::env::args().collect();
    match hided::cli::parse_args(&args).and_then(hided::cli::run) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
