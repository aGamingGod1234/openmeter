use tauri::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateChannel {
    Stable,
    Beta,
}

impl UpdateChannel {
    pub fn from_config(value: Option<&str>) -> Self {
        match value {
            Some("beta") => Self::Beta,
            _ => Self::Stable,
        }
    }
}

pub fn update_endpoint(channel: UpdateChannel) -> Url {
    let endpoint = match channel {
        UpdateChannel::Stable => {
            "https://github.com/aGamingGod1234/openmeter/releases/latest/download/latest.json"
        }
        UpdateChannel::Beta => {
            "https://github.com/aGamingGod1234/openmeter/releases/download/beta/latest.json"
        }
    };
    endpoint
        .parse()
        .expect("hard-coded update endpoint is valid")
}
