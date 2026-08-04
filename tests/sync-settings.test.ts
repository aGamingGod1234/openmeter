import { describe, expect, it } from "vitest";
import {
  deviceStateLabel,
  normalizeSyncSettings,
  recoveryKeyNotice,
  renderSyncSummary,
  shortDeviceRef,
  validHubUrl,
} from "../src/sync-settings";

describe("private sync settings", () => {
  it("defaults off and pins this machine to the Mini PC tailnet hub", () => {
    expect(normalizeSyncSettings({})).toEqual({
      syncEnabled: false,
      syncHubUrl: "http://100.90.87.7:6740",
    });
    expect(validHubUrl("http://100.90.87.7:6740")).toBe(true);
    expect(validHubUrl("http://8.8.8.8:6740")).toBe(false);
    expect(validHubUrl("http://0.0.0.0:6740")).toBe(false);
  });

  it("normal status exposes device health but never recovery or bearer material", () => {
    const summary = renderSyncSummary({
      enabled: true,
      hub_url: "http://100.90.87.7:6740",
      device_id: "device-safe",
      last_success_ms: 1_800_000_000_000,
      has_history_key: true,
      has_device_credential: true,
      tracking_last_success_ms: 1_800_000_000_000,
      device_label: "Laptop",
      protocol_version: "v1+v2",
      devices: [],
    });
    expect(summary).toMatch(/Laptop/);
    expect(summary).not.toMatch(/credential|history.key|recovery.key/i);
  });

  it("warns that a newly generated recovery key is shown only once", () => {
    expect(recoveryKeyNotice()).toMatch(/shown only once/i);
    expect(recoveryKeyNotice()).toMatch(/credential manager/i);
  });

  it("renders delayed and offline health without exposing the full opaque reference", () => {
    expect(deviceStateLabel("delayed")).toBe("Delayed");
    expect(deviceStateLabel("offline")).toBe("Offline");
    expect(shortDeviceRef("device-1234567890abcdef")).toBe("device-1…cdef");
  });
});
