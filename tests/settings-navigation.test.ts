import { describe, expect, it } from "vitest";

import {
  normalizeSettingsSection,
  renderSettingsNav,
  SETTINGS_SECTIONS,
} from "../src/settings-navigation";

describe("settings navigation", () => {
  it("falls back to General for an unknown section", () => {
    expect(normalizeSettingsSection("sync")).toBe("sync");
    expect(normalizeSettingsSection("does-not-exist")).toBe("general");
    expect(normalizeSettingsSection(null)).toBe("general");
  });

  it("renders an accessible labeled navigation rail with one active section", () => {
    const nav = renderSettingsNav("sync");

    expect(SETTINGS_SECTIONS.length).toBeGreaterThan(5);
    expect(nav).toMatch(/aria-label="Settings sections"/);
    expect(nav).toMatch(/data-settings-nav="sync"[^>]*aria-current="page"/);
    expect(nav).toMatch(/data-settings-nav="general"(?![^>]*aria-current="page")/);
    expect(nav).toMatch(/End-to-end encrypted history sync/);
  });
});
