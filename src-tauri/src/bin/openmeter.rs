use openmeter_lib::contracts::serialize_limits;
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
    let output = serialize_limits(&snapshots, chrono::Utc::now().timestamp_millis());
    match serde_json::to_string_pretty(&output) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("openmeter: serialize output: {error}");
            std::process::exit(1);
        }
    }
}
