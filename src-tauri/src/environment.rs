use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct EnvironmentSnapshot {
    home: PathBuf,
    variables: BTreeMap<String, OsString>,
}

impl EnvironmentSnapshot {
    pub fn capture() -> Self {
        Self {
            home: dirs::home_dir().unwrap_or_default(),
            variables: std::env::vars_os()
                .filter_map(|(key, value)| key.into_string().ok().map(|key| (key, value)))
                .collect(),
        }
    }

    pub fn from_values(home: PathBuf, variables: BTreeMap<String, OsString>) -> Self {
        Self { home, variables }
    }

    pub fn var(&self, key: &str) -> Option<&OsStr> {
        self.variables.get(key).map(OsString::as_os_str)
    }

    pub fn home_dir(&self) -> &Path {
        &self.home
    }
}
