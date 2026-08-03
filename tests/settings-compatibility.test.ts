import { describe, expect, it } from "vitest";
import {
  browserCorsWarning,
  normalizeCompatibilitySettings,
} from "../src/settings-compatibility";

describe("compatibility settings", () => {
  it("migrates missing and invalid values to secure stable defaults", () => {
    expect(normalizeCompatibilitySettings({})).toEqual({
      updateChannel: "stable",
      allowBrowserCors: false,
    });
    expect(
      normalizeCompatibilitySettings({
        updateChannel: "nightly",
        allowBrowserCors: "yes",
      }),
    ).toEqual({ updateChannel: "stable", allowBrowserCors: false });
  });

  it("warns that compatibility mode exposes usage to browser pages", () => {
    expect(browserCorsWarning()).toMatch(/any web page/i);
    expect(browserCorsWarning()).toMatch(/usage/i);
  });
});
