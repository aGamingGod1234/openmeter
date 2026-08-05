export const SETTINGS_SECTIONS = [
  {
    id: "general",
    label: "General",
    icon: "◎",
    description: "Refresh, appearance, and dashboard behavior",
  },
  {
    id: "notifications",
    label: "Notifications",
    icon: "◔",
    description: "Warnings before a quota runs low",
  },
  {
    id: "privacy",
    label: "Privacy & security",
    icon: "◈",
    description: "Capture protection and anonymous diagnostics",
  },
  {
    id: "sync",
    label: "Device sync",
    icon: "⇄",
    description: "End-to-end encrypted history sync",
  },
  {
    id: "network",
    label: "Network",
    icon: "⌁",
    description: "Proxy and local API access",
  },
  {
    id: "accounts",
    label: "Accounts",
    icon: "◉",
    description: "Named CLI profiles and card identities",
  },
  {
    id: "api-keys",
    label: "API keys",
    icon: "⌘",
    description: "Credentials stored on this PC",
  },
  {
    id: "updates",
    label: "Updates",
    icon: "↻",
    description: "Release channel and version history",
  },
  {
    id: "advanced",
    label: "Advanced",
    icon: "⚙",
    description: "Compatibility options for integrations",
  },
] as const;

export type SettingsSectionId = (typeof SETTINGS_SECTIONS)[number]["id"];

export function normalizeSettingsSection(value: unknown): SettingsSectionId {
  return SETTINGS_SECTIONS.some((section) => section.id === value)
    ? (value as SettingsSectionId)
    : "general";
}

export function renderSettingsNav(active: SettingsSectionId): string {
  const current = normalizeSettingsSection(active);
  const items = SETTINGS_SECTIONS.map(
    ({ id, label, icon, description }) => `
      <button class="settings-nav-item${id === current ? " active" : ""}" type="button"
        data-settings-nav="${id}" aria-current="${id === current ? "page" : "false"}">
        <span class="settings-nav-icon" aria-hidden="true">${icon}</span>
        <span class="settings-nav-copy">
          <span class="settings-nav-label">${label}</span>
          <span class="settings-nav-description">${description}</span>
        </span>
      </button>`,
  ).join("");

  return `<nav class="settings-nav" aria-label="Settings sections">
    <div class="settings-nav-kicker">Configuration</div>
    <p class="settings-nav-intro">Make OpenMeter fit the way you work.</p>
    <div class="settings-nav-list">${items}</div>
  </nav>`;
}
