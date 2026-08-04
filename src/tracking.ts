export interface TrackingDevice {
  device_id: string;
  label: string;
  generated_at_ms: number;
  received_at_ms: number;
  client_version: string;
  unique_events: number;
  quarantined: boolean;
}

export interface TrackingDay {
  day: string;
  device_id: string;
  device_label: string;
  provider_id: string;
  model: string;
  tokens: number;
  cost: number;
}

export interface TrackingQuota {
  provider_id: string;
  record_id: string;
  metric_id: string;
  used_percent: number;
  remaining: number | null;
  limit: number | null;
  resets_at_ms: number | null;
  observed_at_ms: number;
  source_device_id: string;
  source_device_label: string;
  disagreement: boolean;
}

export interface TrackingDashboard {
  generated_at_ms: number;
  devices: TrackingDevice[];
  days: TrackingDay[];
  quotas: TrackingQuota[];
  legacy_present: boolean;
  unique_events: number;
  quarantined_devices: string[];
}

export type DeviceFilter = "all" | string;
export type TrackingMetric = "tokens" | "spend";
export type TrackingRange = 7 | 30;
export type TrackingGroup = "device" | "provider";

export interface TrackingSeriesOptions {
  range: TrackingRange;
  metric: TrackingMetric;
  groupBy: TrackingGroup;
}

export interface TrackingSeries {
  key: string;
  label: string;
  values: number[];
}

export function filterTracking(
  data: TrackingDashboard,
  deviceId: string,
): TrackingDashboard {
  if (deviceId === "all") {
    return {
      ...data,
      devices: [...data.devices],
      days: [...data.days],
      quotas: [...data.quotas],
      quarantined_devices: [...data.quarantined_devices],
    };
  }
  return {
    ...data,
    devices: data.devices.filter((device) => device.device_id === deviceId),
    days: data.days.filter((row) => row.device_id === deviceId),
    quotas: data.quotas.filter((quota) => quota.source_device_id === deviceId),
    quarantined_devices: data.quarantined_devices.filter((id) => id === deviceId),
    unique_events: data.devices.find((device) => device.device_id === deviceId)?.unique_events ?? 0,
  };
}

export function buildTrackingSeries(
  data: TrackingDashboard,
  options: TrackingSeriesOptions,
): { days: string[]; series: TrackingSeries[] } {
  const availableDays = [...new Set(data.days.map((row) => row.day))].sort();
  const newest = availableDays[availableDays.length - 1];
  const cutoff = newest ? addDays(newest, -(options.range - 1)) : "";
  const days = availableDays.filter((day) => day >= cutoff);
  const dayIndex = new Map(days.map((day, index) => [day, index]));
  const grouped = new Map<string, TrackingSeries>();

  for (const row of data.days) {
    const index = dayIndex.get(row.day);
    if (index === undefined) continue;
    const [key, label] = seriesIdentity(row, options.groupBy);
    const series = grouped.get(key) ?? { key, label, values: days.map(() => 0) };
    series.values[index] += options.metric === "tokens" ? row.tokens : row.cost;
    grouped.set(key, series);
  }

  return {
    days,
    series: [...grouped.values()].sort(
      (left, right) => left.label.localeCompare(right.label) || left.key.localeCompare(right.key),
    ),
  };
}

export function selectQuotaRows(
  data: TrackingDashboard,
  deviceId: string,
): TrackingQuota[] {
  return data.quotas
    .filter((quota) => deviceId === "all" || quota.source_device_id === deviceId)
    .sort(
      (left, right) =>
        left.provider_id.localeCompare(right.provider_id) ||
        left.metric_id.localeCompare(right.metric_id),
    );
}

export function trackingSummary(data: TrackingDashboard): {
  devices: number;
  events: number;
  tokens: number;
  cost: number;
} {
  return {
    devices: data.devices.length,
    events: data.unique_events,
    tokens: data.days.reduce((sum, row) => sum + row.tokens, 0),
    cost: data.days.reduce((sum, row) => sum + row.cost, 0),
  };
}

export function quotaDisagreementCopy(quota: TrackingQuota): string {
  return quota.disagreement
    ? "Devices disagree; showing the newest fresh observation."
    : "";
}

export function escapeTrackingHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

function seriesIdentity(
  row: TrackingDay,
  groupBy: TrackingSeriesOptions["groupBy"],
): [string, string] {
  switch (groupBy) {
    case "device":
      return [row.device_id, row.device_label];
    case "provider":
      return [row.provider_id, row.provider_id];
  }
}

function addDays(day: string, delta: number): string {
  const date = new Date(`${day}T00:00:00Z`);
  date.setUTCDate(date.getUTCDate() + delta);
  return date.toISOString().slice(0, 10);
}
