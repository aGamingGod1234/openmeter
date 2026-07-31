import { describe, expect, it } from "vitest";

import { privacyStatus, privacyTrayValues } from "../src/privacy";

describe("privacy mode", () => {
  it("removes tray values without disabling or mutating provider data", () => {
    const providers = ["claude", "codex"];
    const values = [{ id: "claude", values: [42] }];
    expect(privacyTrayValues(true, values)).toEqual([]);
    expect(privacyTrayValues(false, values)).toEqual(values);
    expect(providers).toEqual(["claude", "codex"]);
    expect(values[0].values).toEqual([42]);
  });

  it("explains the Windows capture behavior", () => {
    expect(privacyStatus(true)).toContain("excluded from screen capture");
  });
});
