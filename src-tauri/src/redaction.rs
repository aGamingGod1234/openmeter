use std::sync::OnceLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use crate::providers::ProviderSnapshot;

pub fn credential_stamp(credential: &[u8]) -> String {
    let digest = Sha256::digest(credential);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn redact_text(input: &str) -> String {
    let mut output = user_path_regex()
        .replace_all(input, "%USERPROFILE%")
        .into_owned();
    output = email_regex().replace_all(&output, "[email]").into_owned();
    output = bearer_regex()
        .replace_all(&output, "${1}[secret]")
        .into_owned();
    output = assignment_regex()
        .replace_all(&output, "${1}[secret]")
        .into_owned();
    token_regex().replace_all(&output, "[secret]").into_owned()
}

pub fn redact_snapshot(snapshot: &mut ProviderSnapshot) {
    snapshot.name = redact_text(&snapshot.name);
    snapshot.plan = snapshot.plan.as_deref().map(redact_text);
    snapshot.error = snapshot.error.as_deref().map(redact_text);
    snapshot.warning = snapshot.warning.as_deref().map(redact_text);
    for metric in &mut snapshot.metrics {
        metric.label = redact_text(&metric.label);
        metric.detail = metric.detail.as_deref().map(redact_text);
        metric.value = metric.value.as_deref().map(redact_text);
    }
}

fn user_path_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"(?i)[a-z]:\\users\\[^\\\s"']+"#).expect("valid user path regex")
    })
}

fn email_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b").expect("valid email regex")
    })
}

fn bearer_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)\b(Bearer\s+)[A-Za-z0-9._~+/-]{12,}=*").expect("valid bearer regex")
    })
}

fn assignment_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?i)\b((?:access[_-]?token|refresh[_-]?token|token|api[_-]?key|authorization)(?:\s*[:=]\s*|\s+)["']?)[A-Za-z0-9._~+/-]{8,}=*["']?"#,
        )
        .expect("valid credential assignment regex")
    })
}

fn token_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:sk-[A-Za-z0-9_-]{16,}|eyJ[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,})\b",
        )
        .expect("valid token regex")
    })
}
