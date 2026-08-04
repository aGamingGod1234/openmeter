import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

const now = Date.now();
const day = new Date().toISOString().slice(0, 10);
const yesterday = shiftDay(day, -1);

if (
  import.meta.env.DEV &&
  new URLSearchParams(location.search).has("trackingPreview")
) {
  mockWindows("main");
  mockIPC((command, args) => {
  switch (command) {
    case "get_config":
      return config();
    case "get_autostart":
      return false;
    case "get_accounts":
      return [];
    case "fetch_usage":
      return snapshots();
    case "fetch_spend":
      return [];
    case "fetch_tracking":
      return tracking();
    case "sync_status":
      return {
        enabled: true,
        hub_url: "http://100.90.87.7:6740",
        device_id: "device-laptop-12345678",
        last_success_ms: now - 60_000,
        has_history_key: true,
        has_device_credential: true,
        tracking_last_success_ms: now - 60_000,
        device_label: "Laptop",
        protocol_version: "v1+v2",
        devices: tracking().devices.map((device) => ({
          device_id: device.device_id,
          label: device.label,
          protocol_version: "v2",
          client_version: device.client_version,
          last_generated_ms: device.generated_at_ms,
          last_received_ms: device.received_at_ms,
          state: "current",
        })),
      };
    case "plugin:app|version":
      return "0.5.0";
    case "plugin:event|listen":
      return 1;
    case "plugin:event|unlisten":
      return null;
    case "set_config":
      return {
        ...config(),
        ...((((args as Record<string, unknown> | undefined)?.patch as object | undefined) ?? {})),
      };
    default:
      return null;
  }
  });
}

function config() {
  return {
    refreshMinutes: 5,
    disabled: [],
    pinned: null,
    trayProviders: [],
    pacingAlways: true,
    telemetry: false,
    notifyAlmostOut: true,
    notifyCuttingClose: true,
    notifyWillRunOut: true,
    spendTab: "today",
    spendMetric: "cost",
    showUsed: false,
    resetExact: false,
    timeFormat: "auto",
    layout: null,
    appearance: "dark",
    density: "compact",
    glassEffects: false,
    shortcut: "",
    privacyMode: false,
    proxy: { enabled: false, url: "" },
    showTotalSpend: true,
    welcomeDismissed: true,
    lastSeenVersion: "0.5.0",
    updateChannel: "stable",
    allowBrowserCors: false,
    syncEnabled: true,
    syncHubUrl: "http://100.90.87.7:6740",
  };
}

function snapshots() {
  return [
    snapshot("codex", "Codex", "Weekly", 41),
    snapshot("claude", "Claude", "Session", 63),
  ];
}

function snapshot(provider: string, name: string, metric: string, used: number) {
  return {
    id: provider,
    provider_id: provider,
    account_id: "default",
    card_id: provider,
    credential_stamp: "opaque",
    account_identity_stamp: "opaque",
    fetched_at: now,
    expires_at: now + 300_000,
    name,
    plan: "Pro",
    status: "ok",
    error: null,
    metrics: [{ label: metric, kind: "progress", used_percent: used, detail: null, value: null, resets_at: now + 3_600_000, period_ms: 604_800_000 }],
    stale: false,
    warning: null,
  };
}

function tracking() {
  const devices = [
    device("device-laptop-12345678", "Laptop", now - 30_000),
    device("device-desktop-abcdef12", "Desktop", now - 90_000),
  ];
  return {
    generated_at_ms: now,
    devices,
    days: [
      row(yesterday, devices[0], "codex", "gpt-5", 860_000, 1.34),
      row(day, devices[0], "claude", "opus-4.1", 540_000, 2.12),
      row(yesterday, devices[1], "claude", "sonnet-4", 710_000, 1.72),
      row(day, devices[1], "codex", "gpt-5.2", 1_240_000, 2.86),
    ],
    quotas: [
      {
        provider_id: "codex",
        record_id: "opaque",
        metric_id: "weekly",
        used_percent: 41,
        remaining: null,
        limit: null,
        resets_at_ms: now + 3_600_000,
        observed_at_ms: now - 60_000,
        source_device_id: devices[1].device_id,
        source_device_label: devices[1].label,
        disagreement: true,
      },
    ],
    legacy_present: false,
    unique_events: 24,
    quarantined_devices: [],
  };
}

function device(device_id: string, label: string, timestamp: number) {
  return { device_id, label, generated_at_ms: timestamp, received_at_ms: timestamp, client_version: "0.5.0", unique_events: 12, quarantined: false };
}

function row(dayValue: string, source: ReturnType<typeof device>, provider_id: string, model: string, tokens: number, cost: number) {
  return { day: dayValue, device_id: source.device_id, device_label: source.label, provider_id, model, tokens, cost };
}

function shiftDay(value: string, delta: number): string {
  const date = new Date(`${value}T00:00:00Z`);
  date.setUTCDate(date.getUTCDate() + delta);
  return date.toISOString().slice(0, 10);
}
