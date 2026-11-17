#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|arg| arg == "--e2e-check") {
        // Lets the test harness reject a production binary before opening a window.
        std::process::exit(if cfg!(feature = "e2e") { 0 } else { 2 });
    }
    if let Err(error) = openzygor::desktop::run() {
        eprintln!("OpenZygor: {error:#}");
        std::process::exit(1);
    }
}
