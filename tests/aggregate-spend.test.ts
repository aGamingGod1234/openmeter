import { describe, expect, it } from "vitest";

import { selectAggregateSpend } from "../src/aggregate-spend";

const window = (cost: number, tokens: number) => ({ cost, tokens, models: [] });

describe("selectAggregateSpend", () => {
  it("uses server-authoritative Codex tokens while keeping Hermes tracking separate", () => {
    const spend = [
      {
        id: "codex",
        name: "Codex",
        today: window(20, 7_900),
        yesterday: window(0, 0),
        last30: window(20, 7_900),
        trend: Array(29).fill(0).concat(7_900),
        unpriced: 0,
        unpriced_models: [],
        authoritative_tokens: true,
      },
    ];
    const tracking = {
      devices: [{ device_id: "laptop" }],
      days: [
        { day: "2026-08-03", provider_id: "codex", model: "gpt-5.6-sol", cost: 2, tokens: 100 },
        { day: "2026-08-03", provider_id: "hermes", model: "gpt-5.6-sol", cost: 1, tokens: 50 },
      ],
    };

    const selected = selectAggregateSpend(spend, tracking, "2026-08-03");

    expect(selected.find((row) => row.id === "codex")?.last30.tokens).toBe(7_900);
    expect(selected.find((row) => row.id === "hermes")?.last30.tokens).toBe(50);
    expect(selected).toHaveLength(2);
  });
});
