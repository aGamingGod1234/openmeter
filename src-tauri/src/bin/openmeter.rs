use openmeter_lib::accounts::AccountRegistry;
use openmeter_lib::contracts::{serialize_limits, serialize_limits_with_registry};
use openmeter_lib::providers::config_dir;
use openmeter_lib::refresh::{refresh_default, CliAction, CliOptions, CLI_HELP};

#[tokio::main]
async fn main() {
    let action = match CliOptions::parse(std::env::args().skip(1)) {
        Ok(action) => action,
        Err(error) => {
            eprintln!("openmeter: {error}\n\n{CLI_HELP}");
            std::process::exit(2);
        }
    };
    let CliAction::Run(options) = action else {
        print!("{CLI_HELP}");
        return;
    };
    let snapshots = refresh_default(options.force, options.filter.as_deref(), &[]).await;
    let generated_at = chrono::Utc::now().timestamp_millis();
    let output = AccountRegistry::load(&config_dir().join("accounts-v1.json"))
        .map(|registry| serialize_limits_with_registry(&snapshots, generated_at, &registry))
        .unwrap_or_else(|_| serialize_limits(&snapshots, generated_at));
    match serde_json::to_string_pretty(&output) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("openmeter: serialize output: {error}");
            std::process::exit(1);
        }
    }
}
