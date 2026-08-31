mod app;
mod layout;
mod pty;
mod render;

fn main() {
    if let Err(error) = app::run() {
        eprintln!("event=spike.failed retryable=false error={error:#?}");
        std::process::exit(1);
    }
}
