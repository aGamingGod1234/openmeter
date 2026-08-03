# Local HTTP API

OpenMeter serves your usage as JSON so your own scripts, widgets, and overlays
can read it.

```
GET http://127.0.0.1:6736/v1/usage          # all enabled providers
GET http://127.0.0.1:6736/v1/usage/:id      # one provider (e.g. /claude)
GET http://127.0.0.1:6736/v1/limits         # normalized limit resources
GET http://127.0.0.1:6736/v1/limits/:id     # one provider or account card
```

Wire format (compatible with the macOS OpenUsage API):

```json
[{
  "providerId": "claude",
  "displayName": "Claude",
  "plan": "Max",
  "fetchedAt": "2026-07-08T01:30:00Z",
  "lines": [{
    "type": "progress",
    "label": "Session",
    "used": 22.0,
    "limit": 100,
    "format": { "kind": "percent" },
    "resetsAt": "2026-07-08T04:39:59Z",
    "periodDurationMs": 18000000
  }]
}]
```

The limits endpoint uses the versioned `openusage.limits.v1` envelope. Provider
cards are keyed by their stable card ID, so multiple accounts from the same
provider remain distinct. UI-only text rows are omitted; progress rows become
normalized resources with usage, remaining capacity, utilization, reset time,
and window length where available.

```json
{
  "schema": "openusage.limits.v1",
  "generatedAt": "2026-07-08T01:30:00.000Z",
  "providers": {
    "codex--work": {
      "providerId": "codex",
      "accountId": "work",
      "displayName": "Codex",
      "plan": "Plus",
      "fetchedAt": "2026-07-08T01:30:00.000Z",
      "expiresAt": "2026-07-08T01:35:00.000Z",
      "stale": false,
      "resources": {
        "weekly": {
          "kind": "consumption",
          "unit": "percent",
          "used": 42.0,
          "limit": 100.0,
          "remaining": 58.0,
          "utilization": 0.42,
          "windowSeconds": 604800.0
        }
      }
    }
  },
  "errors": []
}
```

## Security posture

- **Loopback only.** Binds `127.0.0.1` — nothing on your network can
  reach it.
- **Usage numbers only.** Snapshots of what the dashboard shows — never
  credentials, tokens, or keys.
- **Browser access is off by default.** OpenMeter normally sends no CORS
  headers, so web pages cannot read the API. PowerShell, curl, Rainmeter, and
  native apps are unaffected. To match the macOS compatibility behavior,
  enable **Settings → Advanced → Allow browser pages to read local API**.
  This sends `Access-Control-Allow-Origin: *`, which means any web page you
  visit can read the usage numbers shown by OpenMeter. Credentials and tokens
  are never served.
- **No authentication.** Any program running as your Windows user can
  read this API — that is what makes zero-config widgets and scripts
  possible, and it is a deliberate trade-off. What such a program gets is
  usage percentages and reset times; your credentials are never served.
  (A local process that could steal anything meaningful could also read
  the CLIs' credential files directly — the API adds no new exposure.)
- If port 6736 is already taken, the API is silently unavailable for that
  session; everything else works normally.
