export interface AccountRecord {
  provider_id: string;
  account_id: string;
  card_id: string;
  display_name: string;
  identity_key?: string;
  label?: string;
  sources: Array<{
    id: string;
    kind: "default_home" | "directory" | "manual";
    path?: string;
    holds_default_source: boolean;
  }>;
  enabled: boolean;
}

export function normalizeAccountId(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/--+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}

export function providerDisplayName(providerId: string): string {
  return providerId === "codex" ? "Codex" : providerId === "claude" ? "Claude" : providerId;
}

export function createNamedAccount(
  providerId: string,
  accountId: string,
  label: string,
  directory: string,
): AccountRecord {
  const normalized = normalizeAccountId(accountId);
  const cleanLabel = label.trim();
  const cleanDirectory = directory.trim();
  if (!normalized || normalized === "default") throw new Error("Choose a unique account ID");
  if (!cleanLabel) throw new Error("Enter an account name");
  if (!cleanDirectory) throw new Error("Choose the CLI credential directory");
  return {
    provider_id: providerId,
    account_id: normalized,
    card_id: `${providerId}--${normalized}`,
    display_name: providerDisplayName(providerId),
    label: cleanLabel,
    sources: [
      {
        id: `${providerId}-${normalized}`,
        kind: "directory",
        path: cleanDirectory,
        holds_default_source: false,
      },
    ],
    enabled: true,
  };
}

export function renameAccount(account: AccountRecord, label: string): AccountRecord {
  const cleanLabel = label.trim();
  if (!cleanLabel) throw new Error("Enter an account name");
  return {
    ...account,
    label: cleanLabel,
  };
}

export function resolvedAccountName(account: AccountRecord): string {
  const base = providerDisplayName(account.provider_id);
  const label = account.label?.trim();
  return label ? `${base} — ${label}` : base;
}

export function accountLabel(account: AccountRecord): string {
  return (
    account.label?.trim() ||
    account.display_name.split("—").slice(1).join("—").trim() ||
    account.account_id
  );
}
