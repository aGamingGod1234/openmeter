export const MINI_PC_HUB_URL = "http://100.90.87.7:6740";

export interface SyncSettings {
  syncEnabled: boolean;
  syncHubUrl: string;
}

export interface SyncStatus {
  enabled: boolean;
  hub_url: string;
  device_id: string | null;
  last_success_ms: number | null;
  has_history_key: boolean;
  has_device_credential: boolean;
  tracking_last_success_ms: number | null;
  device_label: string;
  protocol_version: string;
  devices: SyncDeviceSummary[];
}

export interface SyncDeviceSummary {
  device_id: string;
  label: string;
  protocol_version: string;
  client_version: string;
  last_generated_ms: number;
  last_received_ms: number;
  state: "current" | "delayed" | "offline" | "needs_upgrade" | "revoked" | "quarantined";
}

export function normalizeSyncSettings(value: Partial<Record<keyof SyncSettings, unknown>>): SyncSettings {
  const requested = typeof value.syncHubUrl === "string" ? value.syncHubUrl : MINI_PC_HUB_URL;
  return {
    syncEnabled: value.syncEnabled === true,
    syncHubUrl: validHubUrl(requested) ? requested : MINI_PC_HUB_URL,
  };
}

export function validHubUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return (
      url.protocol === "http:" &&
      url.hostname === "100.90.87.7" &&
      url.port === "6740" &&
      url.pathname === "/" &&
      !url.username &&
      !url.password &&
      !url.search &&
      !url.hash
    );
  } catch {
    return false;
  }
}

export function renderSyncSummary(status: SyncStatus): string {
  if (!status.enabled) return "Off — history stays only on this PC.";
  if (!status.device_id) return "Setup incomplete.";
  const last = status.last_success_ms
    ? `Last synced ${new Date(status.last_success_ms).toLocaleString()}`
    : "Not synced yet";
  return `${status.device_label || status.device_id} · ${status.protocol_version} · ${last}`;
}

export function shortDeviceRef(deviceId: string): string {
  return deviceId.length <= 12 ? deviceId : `${deviceId.slice(0, 8)}…${deviceId.slice(-4)}`;
}

export function deviceStateLabel(state: SyncDeviceSummary["state"]): string {
  return state
    .split("_")
    .map((part) => part[0]?.toUpperCase() + part.slice(1))
    .join(" ");
}

export function recoveryKeyNotice(): string {
  return "Shown only once. Save it securely; the working copy is protected by Windows Credential Manager.";
}
