# Upstream parity

OpenMeter 0.5 tracks two MIT-licensed upstreams:

- Pane `272a6ae` supplies the mature Windows tray shell, spend accounting,
  provider integrations, and dashboard.
- OpenUsage `9d2bf09` supplies the account-first behavior and the stable usage
  and limits contracts used as the parity baseline.

The Windows port includes OpenUsage's ten-provider baseline (Claude, Codex,
Cursor, Antigravity, Copilot, Devin, Grok, OpenCode, OpenRouter, and Z.ai),
plus Pane's additional providers. It adds account-isolated caching, named
Claude and Codex credential directories, bounded refreshes, per-provider
cooldowns, loopback `/v1/usage` and `/v1/limits` routes, and the `openmeter`
CLI. The tray retains Pane's local spend, pacing, customization, starred tray
metrics, share cards, updates, notifications, autostart, and proxy support.

Windows-specific additions are capture exclusion, privacy-mode tray hiding,
redacted diagnostics, per-user NSIS installation, and CLI PATH registration.
All application data remains under `%APPDATA%\OpenMeter`; the implementation
does not use or require OneDrive.

When either upstream changes, classify the change as provider behavior,
contract/account behavior, or platform UI. Port it through the corresponding
runtime boundary and extend the existing fixture or contract tests before
changing the Windows shell.
