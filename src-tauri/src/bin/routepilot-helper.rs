#[cfg(target_os = "macos")]
fn main() {
    if std::env::args().nth(1).as_deref() != Some("--daemon") {
        eprintln!("routepilot-helper must be started by its launch daemon");
        std::process::exit(64);
    }

    if let Err(error) = routepilot_lib::vpn::privileged_helper::run_daemon() {
        eprintln!("routepilot-helper failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("routepilot-helper is only available on macOS");
    std::process::exit(64);
}
