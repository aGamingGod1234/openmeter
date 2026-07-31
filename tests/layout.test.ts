import { describe, expect, it } from "vitest";

import { migrateLayout, snapshotCardId } from "../src/layout";

const metric = {
  label: "Session",
  kind: "progress",
  used_percent: 25,
  detail: null,
  value: null,
  resets_at: null,
  period_ms: null,
};

describe("account-aware layout migration", () => {
  it("keeps the default card ID and gives a named account its own layout", () => {
    const defaultClaude = {
      id: "claude",
      provider_id: "claude",
      account_id: "default",
      card_id: "claude",
      name: "Claude",
      plan: "Max",
      status: "ok",
      error: null,
      metrics: [metric],
      stale: false,
      warning: null,
    };
    const workClaude = {
      ...defaultClaude,
      account_id: "work",
      card_id: "claude--work",
      name: "Claude — Work",
    };

    expect(snapshotCardId(defaultClaude)).toBe("claude");
    expect(snapshotCardId(workClaude)).toBe("claude--work");

    const layout = migrateLayout(null, [defaultClaude, workClaude]);
    expect(layout.providerOrder).toEqual(["claude", "claude--work"]);
    expect(Object.keys(layout.providers)).toEqual(["claude", "claude--work"]);
    expect(layout.providers.claude).not.toBe(layout.providers["claude--work"]);
  });
});
