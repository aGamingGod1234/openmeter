import { describe, expect, it } from "vitest";
import {
  buildTrackingSeries,
  escapeTrackingHtml,
  filterTracking,
  quotaDisagreementCopy,
  selectQuotaRows,
  trackingSummary,
  type TrackingDashboard,
} from "../src/tracking";

const data: TrackingDashboard = {
  generated_at_ms: Date.parse("2027-01-15T12:00:00Z"),
  devices: [
    { device_id: "device-laptop", label: "Laptop", generated_at_ms: 1, received_at_ms: 1, client_version: "0.5.0", unique_events: 2, quarantined: false },
    { device_id: "device-desktop", label: "Desktop", generated_at_ms: 2, received_at_ms: 2, client_version: "0.5.0", unique_events: 2, quarantined: false },
  ],
  days: [
    row("2027-01-14", "device-laptop", "Laptop", "codex", "gpt-5", 10, 0.1),
    row("2027-01-15", "device-laptop", "Laptop", "claude", "opus", 20, 0.2),
    row("2027-01-14", "device-desktop", "Desktop", "codex", "gpt-5", 30, 0.3),
    row("2027-01-15", "device-desktop", "Desktop", "claude", "opus", 40, 0.4),
  ],
  quotas: [
    {
      provider_id: "codex",
      record_id: "opaque",
      metric_id: "weekly",
      used_percent: 41,
      remaining: null,
      limit: null,
      resets_at_ms: null,
      observed_at_ms: 2,
      source_device_id: "device-desktop",
      source_device_label: "Desktop",
      disagreement: true,
    },
  ],
  legacy_present: false,
  unique_events: 4,
  quarantined_devices: [],
};

describe("cross-device tracking model", () => {
  it("filters all rows or one selected device without mutating input", () => {
    expect(filterTracking(data, "all").days).toHaveLength(4);
    expect(
      filterTracking(data, "device-laptop").days.every(
        (row) => row.device_id === "device-laptop",
      ),
    ).toBe(true);
    expect(data.days).toHaveLength(4);
  });

  it("builds stable aligned graph series by device", () => {
    const result = buildTrackingSeries(data, {
      range: 7,
      metric: "tokens",
      groupBy: "device",
    });
    expect(result.series.map((series) => series.label)).toEqual(["Desktop", "Laptop"]);
    expect(result.days).toEqual(["2027-01-14", "2027-01-15"]);
    expect(result.series[0].values).toEqual([30, 40]);
  });

  it("selects quota sources and computes the displayed totals", () => {
    expect(selectQuotaRows(data, "all")[0].source_device_label).toBe("Desktop");
    expect(selectQuotaRows(data, "device-laptop")).toEqual([]);
    expect(trackingSummary(data)).toEqual({
      devices: 2,
      events: 4,
      tokens: 100,
      cost: 1,
    });
  });

  it("honors 7/30-day ranges, spend, provider grouping, and zero states", () => {
    const extended = {
      ...data,
      days: [...data.days, row("2026-12-20", "device-laptop", "Laptop", "codex", "gpt-5", 5, 2)],
    };
    const seven = buildTrackingSeries(extended, { range: 7, metric: "spend", groupBy: "provider" });
    const thirty = buildTrackingSeries(extended, { range: 30, metric: "spend", groupBy: "provider" });
    expect(seven.days).not.toContain("2026-12-20");
    expect(thirty.days).toContain("2026-12-20");
    expect(seven.series.map((series) => series.label)).toEqual(["claude", "codex"]);
    expect(buildTrackingSeries({ ...data, days: [] }, { range: 7, metric: "tokens", groupBy: "device" }))
      .toEqual({ days: [], series: [] });
  });

  it("provides safe disagreement copy and escapes untrusted labels", () => {
    expect(quotaDisagreementCopy(data.quotas[0])).toMatch(/devices disagree/i);
    expect(escapeTrackingHtml('<img src=x onerror="boom">')).toBe(
      "&lt;img src=x onerror=&quot;boom&quot;&gt;",
    );
  });
});

function row(
  day: string,
  device_id: string,
  device_label: string,
  provider_id: string,
  model: string,
  tokens: number,
  cost: number,
) {
  return { day, device_id, device_label, provider_id, model, tokens, cost };
}
