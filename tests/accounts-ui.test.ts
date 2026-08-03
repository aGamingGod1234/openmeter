import { describe, expect, it } from "vitest";

import {
  accountLabel,
  createNamedAccount,
  normalizeAccountId,
  renameAccount,
  resolvedAccountName,
} from "../src/accounts-ui";
import { migrateLayout } from "../src/layout";

describe("multi-account settings", () => {
  it("creates and renames a stable sibling card without changing its layout key", () => {
    const account = createNamedAccount(
      "claude",
      "Work Account",
      "Work",
      "D:\\AI\\claude-work",
    );
    expect(account.account_id).toBe("work-account");
    expect(account.card_id).toBe("claude--work-account");
    expect(account.sources).toEqual([
      {
        id: "claude-work-account",
        kind: "directory",
        path: "D:\\AI\\claude-work",
        holds_default_source: false,
      },
    ]);

    const renamed = renameAccount(account, "Company");
    expect(renamed.card_id).toBe(account.card_id);
    expect(renamed.display_name).toBe(account.display_name);
    expect(renamed.label).toBe("Company");
    expect(resolvedAccountName(renamed)).toBe("Claude — Company");
    expect(accountLabel(renamed)).toBe("Company");
  });

  it("rejects reserved IDs and keeps sibling layouts independent", () => {
    expect(normalizeAccountId(" My Team ")).toBe("my-team");
    expect(() =>
      createNamedAccount("codex", "default", "Work", "D:\\codex"),
    ).toThrow();

    const base = {
      id: "claude",
      provider_id: "claude",
      account_id: "default",
      card_id: "claude",
      name: "Claude",
      plan: "Max",
      status: "ok",
      error: null,
      metrics: [],
      stale: false,
      warning: null,
    };
    const layout = migrateLayout(null, [
      base,
      {
        ...base,
        id: "claude--work",
        account_id: "work",
        card_id: "claude--work",
        stale: true,
      },
    ]);
    layout.providers.claude.starred.push("Session");
    expect(layout.providers["claude--work"].starred).toEqual([]);
  });
});
