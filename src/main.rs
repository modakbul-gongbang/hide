fn main() {
    if let Err(error) = herdr_ide::app::run() {
        eprintln!("event=herdr-ide.failed retryable=false error={error:#?}");
        std::process::exit(1);
    }
}
