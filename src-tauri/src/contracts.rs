use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Value};

use crate::providers::{Metric, ProviderSnapshot};

pub const LIMITS_SCHEMA: &str = "openusage.limits.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitResourceKind {
    Consumption,
    Balance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitResourceDescriptor {
    pub key: &'static str,
    pub metric_labels: &'static [&'static str],
    pub kind: LimitResourceKind,
    pub unit: &'static str,
}

pub fn serialize_usage(snapshots: &[ProviderSnapshot]) -> Value {
    Value::Array(snapshots.iter().map(usage_snapshot).collect())
}

pub fn serialize_limits(snapshots: &[ProviderSnapshot], generated_at: i64) -> Value {
    let mut providers = BTreeMap::new();
    let mut errors = Vec::new();

    for snapshot in snapshots {
        let card_id = card_id(snapshot);
        if snapshot.status == "ok" {
            providers.insert(card_id.to_string(), limit_provider(snapshot, generated_at));
            if let Some(message) = snapshot.warning.as_deref() {
                errors.push(WireError {
                    provider_id: card_id.to_string(),
                    message: message.to_string(),
                });
            }
        } else if let Some(message) = snapshot.error.as_deref() {
            errors.push(WireError {
                provider_id: card_id.to_string(),
                message: message.to_string(),
            });
        }
    }

    serde_json::to_value(WireEnvelope {
        schema: LIMITS_SCHEMA,
        generated_at: iso8601(generated_at),
        providers,
        errors,
    })
    .unwrap_or_else(|_| json!({"schema": LIMITS_SCHEMA, "providers": {}, "errors": []}))
}

pub fn descriptors_for(provider_id: &str) -> &'static [LimitResourceDescriptor] {
    match provider_id {
        "claude" => CLAUDE,
        "codex" => CODEX,
        "cursor" => CURSOR,
        "antigravity" => ANTIGRAVITY,
        "copilot" => COPILOT,
        "devin" => DEVIN,
        "grok" => GROK,
        "opencode" => OPENCODE,
        "openrouter" => OPENROUTER,
        "zai" => ZAI,
        _ => &[],
    }
}

fn usage_snapshot(snapshot: &ProviderSnapshot) -> Value {
    let lines = snapshot
        .metrics
        .iter()
        .map(|metric| {
            if metric.kind == "progress" {
                json!({
                    "type": "progress",
                    "label": metric.label,
                    "used": metric.used_percent,
                    "limit": 100,
                    "format": { "kind": "percent" },
                    "resetsAt": metric.resets_at.map(iso8601),
                    "periodDurationMs": metric.period_ms,
                    "color": Value::Null,
                })
            } else {
                json!({
                    "type": "text",
                    "label": metric.label,
                    "value": metric.value,
                    "subtitle": metric.detail,
                    "resetsAt": metric.resets_at.map(iso8601),
                    "color": Value::Null,
                })
            }
        })
        .collect();
    json!({
        "providerId": card_id(snapshot),
        "displayName": snapshot.name,
        "plan": snapshot.plan,
        "lines": lines,
        "fetchedAt": iso8601(snapshot.fetched_at),
    })
}

fn limit_provider(snapshot: &ProviderSnapshot, generated_at: i64) -> WireProvider {
    let provider_id = provider_id(snapshot);
    let expires_at = if snapshot.expires_at > 0 {
        snapshot.expires_at
    } else {
        snapshot.fetched_at + 5 * 60 * 1000
    };
    let mut resources = BTreeMap::new();
    for descriptor in descriptors_for(provider_id) {
        let metric = snapshot.metrics.iter().find(|metric| {
            descriptor
                .metric_labels
                .iter()
                .any(|label| metric.label.eq_ignore_ascii_case(label))
        });
        if let Some(resource) = metric.and_then(|metric| progress_resource(descriptor, metric)) {
            resources.insert(descriptor.key.to_string(), resource);
        }
    }
    WireProvider {
        provider_id: provider_id.to_string(),
        account_id: account_id(snapshot).to_string(),
        display_name: snapshot.name.clone(),
        plan: snapshot.plan.clone(),
        fetched_at: iso8601(snapshot.fetched_at),
        expires_at: iso8601(expires_at),
        stale: snapshot.stale || generated_at >= expires_at,
        resources,
    }
}

fn progress_resource(
    descriptor: &LimitResourceDescriptor,
    metric: &Metric,
) -> Option<WireResource> {
    if metric.kind != "progress" {
        return None;
    }
    let used = metric.used_percent?.max(0.0);
    let limit = 100.0;
    Some(WireResource {
        kind: descriptor.kind,
        unit: descriptor.unit,
        used: (descriptor.kind == LimitResourceKind::Consumption).then_some(used),
        available: (descriptor.kind == LimitResourceKind::Balance).then_some(used),
        limit: Some(limit),
        remaining: Some((limit - used).max(0.0)),
        utilization: Some(used / limit),
        resets_at: metric.resets_at.map(iso8601),
        window_seconds: metric
            .period_ms
            .map(|milliseconds| milliseconds as f64 / 1_000.0),
    })
}

fn card_id(snapshot: &ProviderSnapshot) -> &str {
    if snapshot.card_id.is_empty() {
        &snapshot.id
    } else {
        &snapshot.card_id
    }
}

fn provider_id(snapshot: &ProviderSnapshot) -> &str {
    if snapshot.provider_id.is_empty() {
        &snapshot.id
    } else {
        &snapshot.provider_id
    }
}

fn account_id(snapshot: &ProviderSnapshot) -> &str {
    if snapshot.account_id.is_empty() {
        "default"
    } else {
        &snapshot.account_id
    }
}

fn iso8601(epoch_ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(epoch_ms)
        .map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireEnvelope {
    schema: &'static str,
    generated_at: String,
    providers: BTreeMap<String, WireProvider>,
    errors: Vec<WireError>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireError {
    provider_id: String,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireProvider {
    provider_id: String,
    account_id: String,
    display_name: String,
    plan: Option<String>,
    fetched_at: String,
    expires_at: String,
    stale: bool,
    resources: BTreeMap<String, WireResource>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireResource {
    kind: LimitResourceKind,
    unit: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    used: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    available: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    utilization: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resets_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_seconds: Option<f64>,
}

const fn consumption(
    key: &'static str,
    metric_labels: &'static [&'static str],
) -> LimitResourceDescriptor {
    LimitResourceDescriptor {
        key,
        metric_labels,
        kind: LimitResourceKind::Consumption,
        unit: "percent",
    }
}

const CLAUDE: &[LimitResourceDescriptor] = &[
    consumption("session", &["Session"]),
    consumption("weekly", &["Weekly"]),
    consumption("sonnet", &["Sonnet weekly"]),
    consumption("fable", &["Fable weekly", "Opus weekly"]),
    consumption("extraUsage", &["Extra usage"]),
];
const CODEX: &[LimitResourceDescriptor] = &[
    consumption("session", &["Session"]),
    consumption("weekly", &["Weekly"]),
    consumption("spark", &["Spark"]),
    consumption("sparkWeekly", &["Spark Weekly"]),
];
const CURSOR: &[LimitResourceDescriptor] = &[
    consumption("totalUsage", &["Total usage"]),
    consumption("autoUsage", &["Auto usage"]),
    consumption("apiUsage", &["API usage"]),
    consumption("requests", &["Requests"]),
];
const ANTIGRAVITY: &[LimitResourceDescriptor] = &[
    consumption("geminiSession", &["Gemini session", "Session"]),
    consumption("geminiWeekly", &["Gemini weekly"]),
    consumption("nonGeminiSession", &["Claude", "Non-Gemini session"]),
    consumption("nonGeminiWeekly", &["Non-Gemini weekly"]),
];
const COPILOT: &[LimitResourceDescriptor] = &[
    consumption("premiumCredits", &["Premium requests"]),
    consumption("chat", &["Chat"]),
    consumption("completions", &["Completions"]),
];
const DEVIN: &[LimitResourceDescriptor] = &[
    consumption("daily", &["Daily"]),
    consumption("weekly", &["Weekly"]),
];
const GROK: &[LimitResourceDescriptor] = &[consumption("weekly", &["Usage", "Weekly"])];
const OPENCODE: &[LimitResourceDescriptor] = &[
    consumption("session", &["Session"]),
    consumption("weekly", &["Weekly"]),
    consumption("monthly", &["Monthly"]),
];
const OPENROUTER: &[LimitResourceDescriptor] = &[
    consumption("credits", &["Credits used"]),
    consumption("keyLimit", &["Key limit"]),
];
const ZAI: &[LimitResourceDescriptor] = &[
    consumption("session", &["Session"]),
    consumption("weekly", &["Weekly"]),
    consumption("webSearches", &["Web searches"]),
];
