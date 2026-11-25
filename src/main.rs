use std::sync::{Arc, atomic::AtomicBool};
use webhook_wecom::{Config, Service};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = Config::from_env()?;
    let service = Service::new(config).map_err(|_| "Could not bind LISTEN_ADDR")?;
    service
        .run(Arc::new(AtomicBool::new(false)), std::io::stdout())
        .map_err(|_| "HTTP service failed".into())
}
