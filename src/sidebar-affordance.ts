export function nextSidebarPinnedState(pinned: boolean): boolean {
  return !pinned;
}

export function shouldCloseSidebarOnPointer(
  pinned: boolean,
  pointerInsideSidebar: boolean,
): boolean {
  return pinned && !pointerInsideSidebar;
}

export function sidebarAriaExpanded(pinned: boolean): "true" | "false" {
  return pinned ? "true" : "false";
}
