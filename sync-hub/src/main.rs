use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use openmeter_sync_hub::{router, Hub, Store};

mod service;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("openmeter-sync-hub: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    match arguments.as_slice() {
        [command, rest @ ..] if command == "serve" => serve(parse_options(rest)?).await,
        [group, command, rest @ ..] if group == "enrollment" && command == "create" => {
            create_enrollment(parse_options(rest)?)
        }
        [group, command, rest @ ..] if group == "device" && command == "list" => {
            list_devices(parse_options(rest)?)
        }
        [group, command, rest @ ..] if group == "device" && command == "revoke" => {
            revoke_device(parse_options(rest)?)
        }
        [group, command, rest @ ..] if group == "service" && command == "run" => {
            run_service(parse_options(rest)?)
        }
        _ => Err(cli_error(
            "usage: openmeter-sync-hub <serve|service run|enrollment create|device list|device revoke> [options]",
        )),
    }
}

fn run_service(options: BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
    let bind: SocketAddr = required(&options, "--bind")?.parse()?;
    validate_bind(bind)?;
    let database = required(&options, "--database")?.into();
    let pepper = read_pepper(required(&options, "--pepper-file")?)?;
    service::run(service::ServiceConfig {
        bind,
        database,
        pepper,
    })
}

async fn serve(options: BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
    let bind: SocketAddr = required(&options, "--bind")?.parse()?;
    validate_bind(bind)?;
    let database = required(&options, "--database")?;
    let pepper = read_pepper(required(&options, "--pepper-file")?)?;
    let store = Store::open(database)?;
    store.purge(now_ms()?)?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    println!("OpenMeter Sync Hub listening on {bind}");
    axum::serve(listener, router(Hub::new(store, pepper)))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn create_enrollment(options: BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
    let database = required(&options, "--database")?;
    let pepper = read_pepper(required(&options, "--pepper-file")?)?;
    let hub = Hub::new(Store::open(database)?, pepper);
    println!("{}", hub.create_enrollment(now_ms()?)?);
    Ok(())
}

fn list_devices(options: BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
    let store = Store::open(required(&options, "--database")?)?;
    for device in store.devices()? {
        println!(
            "{}\t{}",
            device.device_id,
            if device.revoked { "revoked" } else { "active" }
        );
    }
    Ok(())
}

fn revoke_device(options: BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
    let store = Store::open(required(&options, "--database")?)?;
    store.revoke(required(&options, "--id")?, now_ms()?)?;
    Ok(())
}

fn parse_options(arguments: &[String]) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    if !arguments.len().is_multiple_of(2) {
        return Err(cli_error("every option requires a value"));
    }
    let mut options = BTreeMap::new();
    for pair in arguments.chunks_exact(2) {
        if !pair[0].starts_with("--") {
            return Err(cli_error("options must start with --"));
        }
        if options.insert(pair[0].clone(), pair[1].clone()).is_some() {
            return Err(cli_error("duplicate option"));
        }
    }
    Ok(options)
}

fn required<'a>(
    options: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, Box<dyn Error>> {
    options
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| cli_error(&format!("missing required option {name}")))
}

fn read_pepper(path: impl AsRef<Path>) -> Result<[u8; 32], Box<dyn Error>> {
    let bytes = fs::read(path)?;
    bytes
        .try_into()
        .map_err(|_| cli_error("pepper file must contain exactly 32 bytes"))
}

fn validate_bind(bind: SocketAddr) -> Result<(), Box<dyn Error>> {
    let allowed = match bind.ip() {
        std::net::IpAddr::V4(ip) => ip.is_private() || is_tailnet(ip),
        std::net::IpAddr::V6(_) => false,
    };
    if !allowed || bind.ip().is_unspecified() || bind.ip().is_loopback() || bind.ip().is_multicast()
    {
        return Err(cli_error(
            "bind address must be a specific private LAN or Tailscale IPv4 address",
        ));
    }
    Ok(())
}

fn is_tailnet(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (octets[1] & 0xc0) == 0x40
}

fn now_ms() -> Result<i64, Box<dyn Error>> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    Ok(i64::try_from(millis)?)
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn cli_error(message: &str) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message))
}
