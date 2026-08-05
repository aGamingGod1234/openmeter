import { describe, expect, it } from "vitest";

import {
  nextSidebarPinnedState,
  shouldCloseSidebarOnPointer,
  sidebarAriaExpanded,
} from "../src/sidebar-affordance";

describe("sidebar affordance", () => {
  it("toggles the pinned state only when the visible handle is clicked", () => {
    expect(nextSidebarPinnedState(false)).toBe(true);
    expect(nextSidebarPinnedState(true)).toBe(false);
  });

  it("closes a pinned sidebar when the dashboard is clicked outside it", () => {
    expect(shouldCloseSidebarOnPointer(true, false)).toBe(true);
    expect(shouldCloseSidebarOnPointer(true, true)).toBe(false);
    expect(shouldCloseSidebarOnPointer(false, false)).toBe(false);
  });

  it("keeps the handle's accessible expanded state in sync", () => {
    expect(sidebarAriaExpanded(false)).toBe("false");
    expect(sidebarAriaExpanded(true)).toBe("true");
  });
});
