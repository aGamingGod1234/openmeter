export function privacyTrayValues<T>(enabled: boolean, values: readonly T[]): T[] {
  return enabled ? [] : [...values];
}

export function privacyStatus(enabled: boolean): string {
  return enabled ? "Privacy mode on — excluded from screen capture" : "Privacy mode off";
}
