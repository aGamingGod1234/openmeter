mod alerts;
pub mod accounts;
pub mod cache;
pub mod contracts;
pub mod credential_store;
pub mod discovery;
pub mod environment;
pub mod httpapi;
pub mod platform;
pub mod refresh;
pub mod redaction;
mod pricing;
pub mod providers;
pub mod spend;
pub mod sync_client;
pub mod sync_history;
mod telemetry;
pub mod updates;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};

// ---------------------------------------------------------------------------
// App settings, stored at %APPDATA%\OpenMeter\config.json
// ---------------------------------------------------------------------------

fn config_path() -> PathBuf {
    providers::config_dir().join("config.json")
}

fn account_registry_path() -> PathBuf {
    providers::config_dir().join("accounts-v1.json")
}

#[tauri::command]
fn get_accounts() -> Result<Vec<accounts::AccountContext>, String> {
    Ok(accounts::AccountRegistry::load(&account_registry_path())?
        .accounts()
        .to_vec())
}

#[tauri::command]
fn save_account(account: accounts::AccountContext) -> Result<(), String> {
    let mut registry = accounts::AccountRegistry::load(&account_registry_path())?;
    registry.upsert(account)?;
    registry.save(&account_registry_path())
}

#[tauri::command]
fn remove_account(card_id: String) -> Result<bool, String> {
    let mut registry = accounts::AccountRegistry::load(&account_registry_path())?;
    let removed = registry.remove(&card_id)?;
    registry.save(&account_registry_path())?;
    Ok(removed)
}

/// A parse failure here once silently reset all settings to defaults, so
/// failures are now logged durably and the last good copy is used instead.
fn note_config_error(context: &str) {
    let line = format!("{} {}\r\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), context);
    let path = providers::config_dir().join("config-error.log");
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
    eprintln!("[openmeter] {context}");
}

fn parse_config_file(path: &PathBuf) -> Result<Value, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
    // Tolerate a UTF-8 BOM (Notepad and PowerShell 5.1 both write one).
    serde_json::from_str(raw.trim_start_matches('\u{feff}')).map_err(|e| format!("parse: {e}"))
}

fn load_config() -> Value {
    let path = config_path();
    if !path.exists() {
        return json!({});
    }
    match parse_config_file(&path) {
        Ok(cfg) => cfg,
        Err(e) => {
            note_config_error(&format!("config.json unreadable ({e}) — trying backup"));
            let backup = providers::config_dir().join("config.json.bak");
            match parse_config_file(&backup) {
                Ok(cfg) => cfg,
                Err(e2) => {
                    note_config_error(&format!("config.json.bak also failed ({e2}) — defaults"));
                    json!({})
                }
            }
        }
    }
}

fn config_with_defaults(mut cfg: Value) -> Value {
    if !cfg.is_object() {
        cfg = json!({});
    }
    let obj = cfg.as_object_mut().unwrap();
    // Out-of-the-box experience: 1-min refresh, pacing always visible,
    // all three quota alerts on, dark + compact. (Autostart defaults on
    // in setup; tray icon defaults to Auto via pinned = null.)
    obj.entry("refreshMinutes").or_insert(json!(1));
    obj.entry("disabled").or_insert(json!([]));
    obj.entry("pinned").or_insert(Value::Null);
    obj.entry("trayProviders").or_insert(json!([]));
    obj.entry("pacingAlways").or_insert(json!(true));
    obj.entry("notifyAlmostOut").or_insert(json!(true));
    obj.entry("notifyCuttingClose").or_insert(json!(true));
    obj.entry("notifyWillRunOut").or_insert(json!(true));
    obj.entry("spendTab").or_insert(json!("today"));
    obj.entry("spendMetric").or_insert(json!("cost"));
    obj.entry("showUsed").or_insert(json!(false));
    obj.entry("resetExact").or_insert(json!(false));
    obj.entry("timeFormat").or_insert(json!("auto"));
    obj.entry("layout").or_insert(Value::Null);
    obj.entry("appearance").or_insert(json!("dark"));
    obj.entry("density").or_insert(json!("compact"));
    obj.entry("glassEffects").or_insert(json!(true));
    obj.entry("shortcut").or_insert(json!(""));
    obj.entry("privacyMode").or_insert(json!(false));
    obj.entry("proxy").or_insert(json!({ "enabled": false, "url": "" }));
    obj.entry("showTotalSpend").or_insert(json!(true));
    obj.entry("welcomeDismissed").or_insert(json!(false));
    // Empty = "never recorded": the frontend uses it to tell a fresh
    // install (no What's-new popup) from an update (popup with the notes).
    obj.entry("lastSeenVersion").or_insert(json!(""));
    // Telemetry defaults ON and must SAY so: without this default the
    // Settings toggle read `undefined` (rendered off) while the sender's
    // own default kept transmitting — a switch that displays off while
    // data flows is the one state a privacy control must never be in.
    obj.entry("telemetry").or_insert(json!(false));
    obj.entry("updateChannel").or_insert(json!("stable"));
    obj.entry("allowBrowserCors").or_insert(json!(false));
    cfg
}

#[tauri::command]
fn get_config() -> Value {
    config_with_defaults(load_config())
}

/// Every key config.json may hold — the same set config_with_defaults seeds.
/// set_config drops anything else so a compromised frontend can't stash
/// arbitrary data in the config file.
const CONFIG_KEYS: &[&str] = &[
    // Not seeded by config_with_defaults (the autostart plugin is the
    // source of truth at runtime) but persisted here so setup() can apply
    // the user's choice on launch.
    "autostart",
    "refreshMinutes",
    "disabled",
    "pinned",
    "trayProviders",
    "pacingAlways",
    "notifyAlmostOut",
    "notifyCuttingClose",
    "notifyWillRunOut",
    "spendMetric",
    "spendTab",
    "showUsed",
    "resetExact",
    "timeFormat",
    "layout",
    "appearance",
    "density",
    "glassEffects",
    "shortcut",
    "privacyMode",
    "proxy",
    "showTotalSpend",
    "welcomeDismissed",
    "lastSeenVersion",
    "telemetry",
    "updateChannel",
    "allowBrowserCors",
];

#[tauri::command]
fn set_config(patch: Value) -> Result<Value, String> {
    let mut cfg = config_with_defaults(load_config());
    if let (Some(target), Some(source)) = (cfg.as_object_mut(), patch.as_object()) {
        for (k, v) in source {
            if CONFIG_KEYS.contains(&k.as_str()) {
                target.insert(k.clone(), v.clone());
            } else {
                eprintln!("[openmeter] set_config: ignoring unknown key '{k}'");
            }
        }
    }
    let dir = providers::config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create config dir: {e}"))?;
    let path = config_path();
    // Keep the last good copy, then write atomically (temp file + rename) so
    // a crash or kill mid-write can never leave a truncated config behind.
    if path.exists() {
        let _ = std::fs::copy(&path, dir.join("config.json.bak"));
    }
    let tmp = dir.join("config.json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&cfg).unwrap_or_default())
        .map_err(|e| format!("write config: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("replace config: {e}"))?;
    httpapi::set_policy(httpapi::ApiPolicy {
        allow_browser_cors: cfg
            .get("allowBrowserCors")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    });
    Ok(cfg)
}

// ---------------------------------------------------------------------------
// Start with Windows
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_autostart(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    // Remember the choice so startup knows whether to re-assert it.
    let _ = set_config(json!({ "autostart": enabled }));
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|e| e.to_string())
    } else {
        manager.disable().map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Tray icon with the pinned metric drawn onto it
// ---------------------------------------------------------------------------

// 4x6 pixel digit font, one nibble per row (bit 3 = leftmost pixel).
const DIGIT_FONT: [[u8; 6]; 10] = [
    [0x6, 0x9, 0x9, 0x9, 0x9, 0x6], // 0
    [0x2, 0x6, 0x2, 0x2, 0x2, 0x7], // 1
    [0x6, 0x9, 0x1, 0x2, 0x4, 0xF], // 2
    [0xE, 0x1, 0x6, 0x1, 0x9, 0x6], // 3
    [0x2, 0x6, 0xA, 0xF, 0x2, 0x2], // 4
    [0xF, 0x8, 0xE, 0x1, 0x9, 0x6], // 5
    [0x6, 0x8, 0xE, 0x9, 0x9, 0x6], // 6
    [0xF, 0x1, 0x2, 0x2, 0x4, 0x4], // 7
    [0x6, 0x9, 0x6, 0x9, 0x9, 0x6], // 8
    [0x6, 0x9, 0x9, 0x7, 0x1, 0x6], // 9
];

/// Renders one or two numbers (0-100) stacked on a 32x32 RGBA tray icon —
/// two rows mimic the Mac menu bar's "100% / 36%" pair. White digits with a
/// black outline so they read on both light and dark taskbars.
fn draw_tray_numbers(values: &[u32]) -> Vec<u8> {
    const SIZE: usize = 32;
    let scale = 2usize;
    let glyph_w = 4 * scale;
    let _glyph_h = 6 * scale;
    let gap = scale;

    let mut mask = [false; SIZE * SIZE];
    let rows: &[usize] = if values.len() >= 2 { &[3, 17] } else { &[10] };

    for (value, y0) in values.iter().zip(rows) {
        let digits: Vec<usize> = value
            .to_string()
            .chars()
            .filter_map(|c| c.to_digit(10).map(|d| d as usize))
            .collect();
        let text_w = digits.len() * glyph_w + digits.len().saturating_sub(1) * gap;
        let x0 = (SIZE.saturating_sub(text_w)) / 2;

        for (i, d) in digits.iter().enumerate() {
            let gx = x0 + i * (glyph_w + gap);
            for (row, bits) in DIGIT_FONT[*d].iter().enumerate() {
                for col in 0..4 {
                    if bits & (0x8 >> col) != 0 {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                let x = gx + col * scale + sx;
                                let y = y0 + row * scale + sy;
                                if x < SIZE && y < SIZE {
                                    mask[y * SIZE + x] = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    // Outline pass: black anywhere adjacent to a text pixel.
    for y in 0..SIZE {
        for x in 0..SIZE {
            if mask[y * SIZE + x] {
                continue;
            }
            let near = (-1i32..=1).any(|dy| {
                (-1i32..=1).any(|dx| {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    nx >= 0
                        && ny >= 0
                        && (nx as usize) < SIZE
                        && (ny as usize) < SIZE
                        && mask[ny as usize * SIZE + nx as usize]
                })
            });
            if near {
                let p = (y * SIZE + x) * 4;
                rgba[p..p + 4].copy_from_slice(&[0, 0, 0, 230]);
            }
        }
    }
    for y in 0..SIZE {
        for x in 0..SIZE {
            if mask[y * SIZE + x] {
                let p = (y * SIZE + x) * 4;
                rgba[p..p + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    rgba
}

/// Picks up to two metrics for the tray icon (like the Mac's stacked pair):
/// the pinned provider's pinned metric first, then its next progress metric.
fn pick_tray_metrics<'a>(
    snapshots: &'a [providers::Snapshot],
    pinned: &Value,
) -> Vec<&'a providers::Metric> {
    let pinned_provider = pinned.get("provider").and_then(Value::as_str);
    let pinned_label = pinned.get("label").and_then(Value::as_str);

    let provider = snapshots
        .iter()
        .find(|s| s.status == "ok" && Some(s.id.as_str()) == pinned_provider)
        .or_else(|| {
            snapshots
                .iter()
                .find(|s| s.status == "ok" && s.metrics.iter().any(|m| m.kind == "progress"))
        });
    let Some(provider) = provider else { return Vec::new() };

    let mut metrics: Vec<&providers::Metric> =
        provider.metrics.iter().filter(|m| m.kind == "progress").collect();
    if let Some(label) = pinned_label {
        if let Some(pos) = metrics.iter().position(|m| m.label == label) {
            metrics.rotate_left(pos);
        }
    }
    metrics.truncate(2);
    metrics
}

fn update_tray(app: &tauri::AppHandle, snapshots: &[providers::Snapshot], cfg: &Value) {
    let Some(tray) = app.tray_by_id("tray") else {
        return;
    };
    if cfg
        .get("privacyMode")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let _ = tray.set_tooltip(Some("OpenMeter - Privacy mode"));
        if let Some(default) = app.default_window_icon() {
            let _ = tray.set_icon(Some(default.clone()));
        }
        return;
    }

    let mut tooltip = String::from("OpenMeter");
    for s in snapshots.iter().filter(|s| s.status == "ok").take(6) {
        if let Some(m) = s.metrics.iter().find(|m| m.kind == "progress") {
            let left = (100.0 - m.used_percent.unwrap_or(0.0)).clamp(0.0, 100.0).round();
            tooltip.push_str(&format!("\n{} {}: {left:.0}% left", s.name, m.label));
        }
    }
    let _ = tray.set_tooltip(Some(&tooltip));

    // When the Mac-style tray strip is active it carries the numbers, so the
    // main icon stays the app logo (the strip icons are per-provider).
    let strip_active = cfg
        .get("trayProviders")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty());
    if strip_active {
        if let Some(default) = app.default_window_icon() {
            let _ = tray.set_icon(Some(default.clone()));
        }
        return;
    }

    let metrics = pick_tray_metrics(snapshots, cfg.get("pinned").unwrap_or(&Value::Null));
    if !metrics.is_empty() {
        let lefts: Vec<u32> = metrics
            .iter()
            .map(|m| (100.0 - m.used_percent.unwrap_or(0.0)).clamp(0.0, 100.0).round() as u32)
            .collect();
        let icon = tauri::image::Image::new_owned(draw_tray_numbers(&lefts), 32, 32);
        let _ = tray.set_icon(Some(icon));
    }
}

// ---------------------------------------------------------------------------
// Mac-style tray strip: a [provider logo][live numbers] icon pair per
// selected provider. The UI rasterizes each SVG logo to 32x32 RGBA (the
// webview already has the icons) and sends the pixels here.
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct StripEntry {
    id: String,
    logo: Vec<u8>, // 32x32 RGBA
    values: Vec<u32>,
    tooltip: String,
}

/// Every provider id that may appear in the tray strip. Doubles as the
/// allowlist for update_tray_strip: ids from the frontend are validated
/// against this before being spliced into tray icon ids, and stale strip
/// icons are removed for exactly this set.
const STRIP_PROVIDER_IDS: [&str; 18] = [
    "claude",
    "codex",
    "cursor",
    "opencode",
    "copilot",
    "grok",
    "devin",
    "minimax",
    "openrouter",
    "zai",
    "antigravity",
    "deepseek",
    "moonshot",
    "elevenlabs",
    "ollama",
    "codebuff",
    "kilo",
    "aihubmix",
];

#[tauri::command]
fn update_tray_strip(app: tauri::AppHandle, entries: Vec<StripEntry>) -> Result<(), String> {
    let entries = if config_with_defaults(load_config())
        .get("privacyMode")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        Vec::new()
    } else {
        entries
    };
    let handle = app.clone();
    app.run_on_main_thread(move || {
        // Remove strip icons for providers no longer selected.
        for id in STRIP_PROVIDER_IDS {
            if !entries.iter().any(|e| e.id == id) {
                let _ = handle.remove_tray_by_id(&format!("strip-logo-{id}"));
                let _ = handle.remove_tray_by_id(&format!("strip-num-{id}"));
            }
        }

        for entry in &entries {
            // Only known provider ids may reach the tray icon namespace.
            if !STRIP_PROVIDER_IDS.contains(&entry.id.as_str()) {
                continue;
            }
            if entry.logo.len() != 32 * 32 * 4 {
                continue;
            }
            let logo_id = format!("strip-logo-{}", entry.id);
            let num_id = format!("strip-num-{}", entry.id);
            let logo_icon = tauri::image::Image::new_owned(entry.logo.clone(), 32, 32);
            let num_icon =
                tauri::image::Image::new_owned(draw_tray_numbers(&entry.values), 32, 32);

            if let Some(tray) = handle.tray_by_id(&num_id) {
                let _ = tray.set_icon(Some(num_icon));
                let _ = tray.set_tooltip(Some(&entry.tooltip));
                if let Some(logo_tray) = handle.tray_by_id(&logo_id) {
                    let _ = logo_tray.set_tooltip(Some(&entry.tooltip));
                }
                continue;
            }

            // New pair — numbers first: Windows inserts each new tray icon
            // to the LEFT of the previous one, so creating numbers → logo
            // lands as "logo | numbers" left-to-right, like the Mac strip.
            for (tray_id, icon) in [(num_id, num_icon), (logo_id, logo_icon)] {
                let _ = TrayIconBuilder::with_id(tray_id)
                    .icon(icon)
                    .tooltip(&entry.tooltip)
                    .show_menu_on_left_click(false)
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            position,
                            ..
                        } = event
                        {
                            toggle_popover(tray.app_handle(), position);
                        }
                    })
                    .build(&handle);
            }
        }
    })
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Usage fetching
// ---------------------------------------------------------------------------

/// Called by the UI. Refreshes every enabled provider at the same time and
/// returns whatever each one found — data, "not signed in", or an error.
#[tauri::command]
async fn fetch_usage(app: tauri::AppHandle) -> Vec<providers::Snapshot> {
    let cfg = config_with_defaults(load_config());
    let disabled: Vec<String> = cfg
        .get("disabled")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();

    let mut all = refresh::refresh_default(false, None, &disabled).await;
    if let Ok(registry) = accounts::AccountRegistry::load(&account_registry_path()) {
        for snapshot in &mut all {
            snapshot.name = registry.resolve_name(&snapshot.card_id, &snapshot.name);
        }
    }
    let enabled_ids: Vec<String> = all
        .iter()
        .map(|snapshot| snapshot.provider_id.clone())
        .collect();


    httpapi::publish(&all);
    update_tray(&app, &all, &cfg);

    // Anonymous daily-rollup telemetry (Settings → "Share anonymous usage
    // statistics"). Fire-and-forget: it must never delay or fail a refresh.
    {
        let enabled = cfg.get("telemetry").and_then(Value::as_bool).unwrap_or(false);
        let starred_metrics: Vec<String> = cfg
            .pointer("/layout/providers")
            .and_then(Value::as_object)
            .map(|provs| {
                provs
                    .iter()
                    .flat_map(|(pid, entry)| {
                        entry
                            .get("starred")
                            .and_then(Value::as_array)
                            .map(|a| {
                                a.iter()
                                    .filter_map(Value::as_str)
                                    .map(|m| format!("{pid}/{m}"))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .unwrap_or_default();
        let snap = telemetry::ConfigSnapshot {
            app_version: app.package_info().version.to_string(),
            enabled_providers: enabled_ids,
            starred_metrics,
            appearance: cfg
                .get("appearance")
                .and_then(Value::as_str)
                .unwrap_or("system")
                .to_string(),
            density: cfg
                .get("density")
                .and_then(Value::as_str)
                .unwrap_or("regular")
                .to_string(),
            refresh_minutes: cfg.get("refreshMinutes").and_then(Value::as_u64).unwrap_or(5),
        };
        let outcomes: Vec<telemetry::Outcome> = all
            .iter()
            .map(|s| telemetry::Outcome {
                id: s.id.clone(),
                status: s.status.clone(),
                stale: s.stale,
                error: s.error.clone().or_else(|| s.warning.clone()),
            })
            .collect();
        tauri::async_runtime::spawn(telemetry::record(enabled, snap, outcomes));
    }

    for alert in alerts::evaluate(&all, &cfg) {
        use tauri_plugin_notification::NotificationExt;
        let _ = app
            .notification()
            .builder()
            .title(&alert.title)
            .body(&alert.body)
            .show();
    }

    all
}

/// Computes local spend (Today / Yesterday / Last 30 Days) from the CLIs'
/// own session logs. Heavy file IO, so it runs on a blocking thread.
#[tauri::command]
async fn fetch_spend() -> Vec<spend::ProviderSpend> {
    eprintln!("[openmeter] spend: scan starting");
    let started = std::time::Instant::now();
    // Cursor's CSV export needs the async client; fetch it here and hand it
    // to the blocking scan.
    let cursor_csv = providers::cursor::fetch_usage_csv().await;
    let registry = accounts::AccountRegistry::load(&account_registry_path()).unwrap_or_default();
    let environment = environment::launch_environment().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        spend::collect_for_accounts(cursor_csv, &registry, &environment)
    })
        .await
        .unwrap_or_default();
    eprintln!(
        "[openmeter] spend: {} providers in {:?}",
        result.len(),
        started.elapsed()
    );
    result
}

/// Saves (or clears, when `key` is empty) a user-pasted API key to
/// %APPDATA%\OpenMeter\<provider>.json.
#[tauri::command]
fn set_api_key(provider: String, key: String) -> Result<(), String> {
    if !matches!(
        provider.as_str(),
        "openrouter"
            | "zai"
            | "minimax"
            | "deepseek"
            | "moonshot"
            | "elevenlabs"
            | "codebuff"
            | "kilo"
            | "aihubmix"
    ) {
        return Err(format!("unknown provider: {provider}"));
    }
    let dir = providers::config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create config dir: {e}"))?;
    let path = dir.join(format!("{provider}.json"));
    let key = key.trim();
    if key.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    std::fs::write(&path, serde_json::json!({ "apiKey": key }).to_string())
        .map_err(|e| format!("write key file: {e}"))
}

/// Opens a provider quick link in the default browser. Only plain web URLs —
/// nothing that could launch a program.
#[tauri::command]
fn open_link(app: tauri::AppHandle, url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only http(s) links allowed".into());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("open link: {e}"))
}

/// Puts a share-card PNG (rendered by the frontend on a canvas) onto the
/// Windows clipboard as a real image.
#[tauri::command]
fn copy_share_image(png_base64: String) -> Result<(), String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png_base64.trim())
        .map_err(|e| format!("decode png: {e}"))?;
    let img = tauri::image::Image::from_bytes(&bytes).map_err(|e| format!("parse png: {e}"))?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    let rgba = img.rgba().to_vec();
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("clipboard: {e}"))?;
    clipboard
        .set_image(arboard::ImageData { width: w, height: h, bytes: rgba.into() })
        .map_err(|e| format!("copy image: {e}"))
}

/// (Re-)registers the global toggle-popover shortcut. An empty string clears
/// it. The accelerator uses Tauri syntax, e.g. "Ctrl+Shift+U".
fn register_shortcut(app: &tauri::AppHandle, accel: &str) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let privacy: Shortcut = "Ctrl+Shift+P".parse().expect("static privacy shortcut");
    gs.on_shortcut(privacy, |app, _shortcut, event| {
        if event.state() == ShortcutState::Pressed {
            let enabled = !config_with_defaults(load_config())
                .get("privacyMode")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let _ = apply_privacy_mode(app, enabled);
            let _ = app.emit("privacy-mode-changed", enabled);
        }
    })
    .map_err(|e| format!("register privacy shortcut: {e}"))?;
    let accel = accel.trim();
    if accel.is_empty() {
        return Ok(());
    }
    let shortcut: Shortcut = accel
        .parse()
        .map_err(|_| format!("could not parse shortcut \"{accel}\""))?;
    gs.on_shortcut(shortcut, |app, _shortcut, event| {
        if event.state() == ShortcutState::Pressed {
            let pos = app
                .cursor_position()
                .unwrap_or(tauri::PhysicalPosition::new(1200.0, 700.0));
            toggle_popover(app, pos);
        }
    })
    .map_err(|e| format!("register shortcut: {e}"))
}

#[tauri::command]
fn set_shortcut(app: tauri::AppHandle, shortcut: String) -> Result<(), String> {
    register_shortcut(&app, &shortcut)
}

fn apply_privacy_mode(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    if let Some(window) = app.get_webview_window("main") {
        let hwnd = window
            .hwnd()
            .map_err(|error| format!("window handle: {error}"))?;
        platform::set_capture_exclusion(hwnd, enabled)?;
    }
    let cfg = set_config(json!({ "privacyMode": enabled }))?;
    update_tray(app, &[], &cfg);
    if enabled {
        let _ = update_tray_strip(app.clone(), Vec::new());
    }
    Ok(())
}

#[tauri::command]
fn set_privacy_mode(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    apply_privacy_mode(&app, enabled)
}

#[tauri::command]
fn copy_diagnostics(app: tauri::AppHandle) -> Result<(), String> {
    let cfg = config_with_defaults(load_config());
    let registry = accounts::AccountRegistry::load(&account_registry_path()).unwrap_or_default();
    let accounts: Vec<Value> = registry
        .accounts()
        .iter()
        .map(|account| {
            json!({
                "providerId": account.provider_id,
                "accountId": account.account_id.as_str(),
                "cardId": account.card_id,
                "enabled": account.enabled,
                "sources": account.sources.iter().map(|source| json!({
                    "id": source.id,
                    "kind": source.kind,
                    "holdsDefaultSource": source.holds_default_source,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let document = json!({
        "app": "OpenMeter",
        "version": app.package_info().version.to_string(),
        "platform": "Windows 11",
        "settings": {
            "refreshMinutes": cfg.get("refreshMinutes"),
            "disabled": cfg.get("disabled"),
            "privacyMode": cfg.get("privacyMode"),
            "telemetry": cfg.get("telemetry"),
            "appearance": cfg.get("appearance"),
            "density": cfg.get("density"),
        },
        "accounts": accounts,
    });
    let text = redaction::redact_text(
        &serde_json::to_string_pretty(&document)
            .map_err(|error| format!("serialize diagnostics: {error}"))?,
    );
    arboard::Clipboard::new()
        .map_err(|error| format!("clipboard: {error}"))?
        .set_text(text)
        .map_err(|error| format!("copy diagnostics: {error}"))
}

/// Spends one banked Codex rate-limit reset credit. Irreversible — the
/// frontend shows a confirm dialog before calling this.
#[tauri::command]
async fn codex_redeem_credit(credit_id: String) -> Result<String, String> {
    providers::codex::redeem_credit(&credit_id).await
}

/// OpenMeter reads its signed update manifest from this repository's latest
/// GitHub release. The public verification key remains in tauri.conf.json.
fn build_updater(
    app: &tauri::AppHandle,
) -> Result<tauri_plugin_updater::Updater, String> {
    use tauri_plugin_updater::UpdaterExt;
    let config = config_with_defaults(load_config());
    let channel = updates::UpdateChannel::from_config(
        config.get("updateChannel").and_then(Value::as_str),
    );
    let endpoints = vec![updates::update_endpoint(channel)];
    app.updater_builder()
        .endpoints(endpoints)
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())
}

/// Downloads and installs a pending update, then restarts the app. Only
/// called from the frontend banner after check_for_update announced one.
#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    let updater = build_updater(&app)?;
    match updater.check().await.map_err(|e| e.to_string())? {
        Some(update) => {
            update
                .download_and_install(|_, _| {}, || {})
                .await
                .map_err(|e| e.to_string())?;
            app.restart();
        }
        // The update the button promised is gone (yanked release, CDN
        // hiccup). Succeeding silently would strand the frontend in its
        // "Installing…" state — fail so the button can recover.
        None => Err("update no longer available — try again shortly".into()),
    }
}

/// Popover-open update check: the footer asks on every tray click and
/// shows an Update button when this returns a newer version.
#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let updater = build_updater(&app)?;
    updater
        .check()
        .await
        .map(|u| u.map(|u| u.version.clone()))
        .map_err(|e| e.to_string())
}

/// Startup + every 4 h: quiet update check; a hit emits "update-available"
/// with the new version so the frontend can show its banner. 404 (no
/// releases yet) and offline are non-events.
fn spawn_update_checker(app: &tauri::AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if let Ok(updater) = build_updater(&handle) {
                match updater.check().await {
                    Ok(Some(update)) => {
                        let _ = handle.emit("update-available", update.version.clone());
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("[openmeter] update check: {e}"),
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(4 * 3600)).await;
        }
    });
}

// ---------------------------------------------------------------------------
// Tray + popover window plumbing
// ---------------------------------------------------------------------------

// Clicking the tray icon while the popover is open first steals focus
// (which hides the window) and then delivers the click event. Without a
// guard, that click would instantly re-open the window the user just
// closed. We remember when the last auto-hide happened and ignore tray
// clicks that arrive right after it.
static LAST_AUTO_HIDE_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Tells WebView2 to release memory while the popover is hidden and return
/// to normal when it shows. Tauri doesn't expose wry's setter for this, so
/// we make the same COM calls wry does (SetMemoryUsageTargetLevel).
fn set_webview_memory_level(window: &tauri::WebviewWindow, low: bool) {
    let _ = window.with_webview(move |webview| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL,
        };
        use windows_core::Interface;
        if let Ok(core) = webview.controller().CoreWebView2() {
            if let Ok(wv19) = core.cast::<ICoreWebView2_19>() {
                let level = COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL(if low { 1 } else { 0 });
                let _ = wv19.SetMemoryUsageTargetLevel(level);
            }
        }
    });
}

fn toggle_popover(app: &tauri::AppHandle, click: tauri::PhysicalPosition<f64>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        set_webview_memory_level(&window, true);
        return;
    }

    if now_ms().saturating_sub(LAST_AUTO_HIDE_MS.load(Ordering::Relaxed)) < 300 {
        return;
    }

    set_webview_memory_level(&window, false);

    // Anchor the popover's bottom-right corner near the tray click,
    // which sits next to the clock on a standard bottom taskbar.
    let size = window
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(380, 600));
    let x = (click.x - f64::from(size.width)).max(0.0);
    let y = (click.y - f64::from(size.height) - 8.0).max(0.0);
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("popover-shown", ());
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = environment::install_launch_environment(environment::EnvironmentSnapshot::capture());
    tauri::Builder::default()
        // Second launches just poke the existing instance's popover open
        // instead of spawning a duplicate tray icon (Mac parity).
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            let pos = app
                .cursor_position()
                .unwrap_or(tauri::PhysicalPosition::new(1200.0, 700.0));
            toggle_popover(app, pos);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            fetch_usage,
            get_accounts,
            save_account,
            remove_account,
            fetch_spend,
            set_api_key,
            get_config,
            set_config,
            get_autostart,
            set_autostart,
            update_tray_strip,
            open_link,
            copy_share_image,
            set_shortcut,
            set_privacy_mode,
            copy_diagnostics,
            codex_redeem_credit,
            install_update,
            check_update
        ])
        .setup(|app| {
            spawn_update_checker(app.handle());
            let quit = MenuItem::with_id(app, "quit", "Quit OpenMeter", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit])?;

            TrayIconBuilder::with_id("tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("OpenMeter")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "quit" {
                        app.exit(0);
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        position,
                        ..
                    } = event
                    {
                        toggle_popover(tray.app_handle(), position);
                    }
                })
                .build(app)?;

            // The popover starts hidden, so start the webview in low-memory
            // mode too; it flips to normal the first time it is shown.
            if let Some(wv) = app.get_webview_window("main") {
                set_webview_memory_level(&wv, true);
            }

            let config = config_with_defaults(load_config());
            httpapi::set_policy(httpapi::ApiPolicy {
                allow_browser_cors: config
                    .get("allowBrowserCors")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
            httpapi::start();

            let saved_shortcut = load_config()
                .get("shortcut")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if let Err(e) = register_shortcut(app.handle(), &saved_shortcut) {
                eprintln!("[openmeter] shortcut: {e}");
            }
            let privacy_mode = config_with_defaults(load_config())
                .get("privacyMode")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if let Err(error) = apply_privacy_mode(app.handle(), privacy_mode) {
                eprintln!("[openmeter] privacy mode: {error}");
            }

            // Start with Windows is on by default (like the Mac app's
            // launch-at-login) and re-asserted each launch so the registry
            // entry follows the exe if it moves — e.g. loose exe → installed.
            // Only an explicit "off" in Settings is respected. Skipped in dev
            // builds so the debug exe never registers itself.
            if !cfg!(debug_assertions) {
                let wants_autostart = load_config()
                    .get("autostart")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                if wants_autostart {
                    use tauri_plugin_autostart::ManagerExt;
                    let _ = app.autolaunch().enable();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::Focused(false) = event {
                    if window.hide().is_ok() {
                        LAST_AUTO_HIDE_MS.store(now_ms(), Ordering::Relaxed);
                        if let Some(wv) = window.app_handle().get_webview_window("main") {
                            set_webview_memory_level(&wv, true);
                        }
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
