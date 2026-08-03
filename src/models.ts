export interface Metric {
  label: string;
  kind: string;
  used_percent: number | null;
  detail: string | null;
  value: string | null;
  resets_at: number | null;
  period_ms: number | null;
}

export interface Snapshot {
  /** Legacy provider identity; retained for compatibility with existing builds. */
  id: string;
  /** Stable provider family for account-qualified cards. */
  provider_id?: string;
  /** Stable account identity without secrets. */
  account_id?: string;
  /** Unique UI/layout identity, such as `claude--work`. */
  card_id?: string;
  name: string;
  plan: string | null;
  status: string;
  error: string | null;
  metrics: Metric[];
  stale: boolean;
  warning: string | null;
}

export type { AccountRecord } from "./accounts-ui";
export type UpdateChannel = "stable" | "beta";

export function snapshotProviderId(snapshot: Snapshot): string {
  return snapshot.provider_id?.trim() || snapshot.id;
}
