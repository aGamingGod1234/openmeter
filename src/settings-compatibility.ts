import type { UpdateChannel } from "./models";

export interface CompatibilitySettings {
  updateChannel: UpdateChannel;
  allowBrowserCors: boolean;
}

export function normalizeCompatibilitySettings(
  value: Partial<Record<keyof CompatibilitySettings, unknown>>,
): CompatibilitySettings {
  return {
    updateChannel: value.updateChannel === "beta" ? "beta" : "stable",
    allowBrowserCors: value.allowBrowserCors === true,
  };
}

export function browserCorsWarning(): string {
  return "Compatibility mode lets any web page you visit read the usage numbers from OpenMeter's local API. Credentials are never exposed.";
}
