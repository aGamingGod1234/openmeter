use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{mpsc, OnceLock};
use std::time::Duration;

use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

use openmeter_sync_hub::{router, Hub, Store};

const SERVICE_NAME: &str = "OpenMeterSyncHub";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

#[derive(Clone)]
pub struct ServiceConfig {
    pub bind: SocketAddr,
    pub database: PathBuf,
    pub pepper: [u8; 32],
}

static CONFIG: OnceLock<ServiceConfig> = OnceLock::new();

define_windows_service!(service_entry, service_main);

pub fn run(config: ServiceConfig) -> Result<(), Box<dyn std::error::Error>> {
    CONFIG
        .set(config)
        .map_err(|_| "service configuration was already initialized")?;
    service_dispatcher::start(SERVICE_NAME, service_entry)?;
    Ok(())
}

fn service_main(_arguments: Vec<OsString>) {
    if let Err(error) = run_service() {
        if let Some(config) = CONFIG.get() {
            let path = config
                .database
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("service-error.log");
            let _ = std::fs::write(path, format!("OpenMeter Sync Hub stopped: {error}\r\n"));
        }
    }
}

fn run_service() -> Result<(), Box<dyn std::error::Error>> {
    let config = CONFIG
        .get()
        .cloned()
        .ok_or("service configuration is unavailable")?;
    let (shutdown_tx, shutdown_rx) = mpsc::channel();
    let event_handler = move |event| match event {
        ServiceControl::Stop => {
            let _ = shutdown_tx.send(());
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let status = service_control_handler::register(SERVICE_NAME, event_handler)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let store = Store::open(&config.database)?;
    let listener = runtime.block_on(tokio::net::TcpListener::bind(config.bind))?;
    status.set_service_status(service_status(
        ServiceState::Running,
        ServiceControlAccept::STOP,
        ServiceExitCode::Win32(0),
    ))?;
    let hub = Hub::new(store, config.pepper);
    let result = runtime.block_on(async move {
        axum::serve(listener, router(hub))
            .with_graceful_shutdown(async move {
                let _ = tokio::task::spawn_blocking(move || shutdown_rx.recv()).await;
            })
            .await
    });
    let exit_code = if result.is_ok() { 0 } else { 1 };
    status.set_service_status(service_status(
        ServiceState::Stopped,
        ServiceControlAccept::empty(),
        ServiceExitCode::Win32(exit_code),
    ))?;
    result?;
    Ok(())
}

fn service_status(
    state: ServiceState,
    accepted: ServiceControlAccept,
    exit_code: ServiceExitCode,
) -> ServiceStatus {
    ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: state,
        controls_accepted: accepted,
        exit_code,
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    }
}
