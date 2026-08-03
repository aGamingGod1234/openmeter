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
  return `${status.device_id} · ${last}`;
}

export function recoveryKeyNotice(): string {
  return "Shown only once. Save it securely; the working copy is protected by Windows Credential Manager.";
}
