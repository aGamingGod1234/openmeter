# Privacy

OpenMeter is built on one rule: **your data is nobody's business, including
ours.** There is no OpenMeter server and no OpenMeter account. Optional
anonymous daily statistics are disabled by default and require explicit opt-in.

## Every network call OpenMeter can make

This is the complete list. Anything not listed here does not happen.

| Destination | When | What is sent |
|---|---|---|
| Each provider's own API (Anthropic, OpenAI/ChatGPT, cursor.com, GitHub, x.ai, Devin, MiniMax, OpenRouter, Z.ai, Google, DeepSeek, Moonshot, ElevenLabs, Codebuff, Kilo…) | Every refresh (default 1 min), only for providers you have enabled | That provider's own token/key, exactly as its official tool would send it. Full per-provider detail: [providers.md](providers.md) |
| `raw.githubusercontent.com` (LiteLLM), `models.dev`, `robinebers.github.io` | ~Daily | Anonymous GET for public model price tables (no identifying data) |
| `github.com/aGamingGod1234/openmeter/releases` | On launch + every 4 h | Anonymous GET for the signed update manifest. |
| `us.i.posthog.com` | At most once per day, only after explicit opt-in | The two anonymous daily-statistic events described below — a random ID, version, enabled-provider list, and per-provider success/failure counts. Never usage amounts, spend, keys, or error text. |
| `127.0.0.1:11434` (your own PC) | Every refresh, if Ollama is enabled | Local-only query of your Ollama server |

Notably absent: session recording, event streams, A/B flags, autocapture
of any kind — none of it exists in the codebase, and the daily statistic
above is the entire analytics surface.

## Anonymous usage statistics

Settings → Privacy → **"Share anonymous usage statistics"** is off by
default. Turning it off is a hard stop — nothing is counted, nothing is
written, and the stored random ID is deleted, so re-enabling starts over
as a brand-new anonymous install).

When opted in, OpenMeter sends at most two kinds of event per day to PostHog:

- **`app_daily_active`** — once per day: "this install was alive today",
  the app version, which providers are enabled, which metrics you
  starred (stable IDs only), appearance/density/refresh settings.
- **`provider_refresh_daily`** — per provider, summarizing the previous
  day: how many refreshes succeeded, went stale, or failed, with failure
  *categories* only (auth / rate-limit / server / network / other).
  The raw error text never leaves your machine — it can contain paths
  or account details, so only the category enum is sent.

The identity attached to these events is a **random UUID** generated on
your machine — derived from nothing (not your hardware, not your IP, not
your account), linked to nothing, stored in `%APPDATA%\OpenMeter\telemetry.json`.
Every event also instructs PostHog not to build a person profile, and the
PostHog project is configured to **discard client IP addresses** at
ingestion (country-level GeoIP resolves first, then the IP is dropped).

What is *never* sent, with this toggle on or off: your quotas, usage
percentages, spend amounts, model names from your logs, tokens, keys,
file paths, or any free-form text. The entire implementation is one
auditable file: [`src-tauri/src/telemetry.rs`](../src-tauri/src/telemetry.rs)
— no SDK, just two documented POSTs.

## The update check

OpenMeter fetches `latest.json` from this repository's latest GitHub release.
The manifest is public, and downloaded installers are verified against the
updater public key baked into the app. OpenMeter adds no update telemetry,
machine identifier, username, quota, spend, or provider data to the request.

## What stays on your PC

- **Credentials**: read from the files the official CLIs already maintain
  (see [providers.md](providers.md)); pasted API keys live in
  `%APPDATA%\OpenMeter\<provider>.json`. Sent only to their own vendor.
- **Refreshed OAuth tokens**: written back to the CLIs' own credential
  files so your tools stay signed in — same behavior as the CLIs
  themselves.
- **Usage snapshots & spend cache**: `%APPDATA%\OpenMeter\` — cached locally so
  the app opens instantly; never uploaded.
- **Spend accounting**: computed by reading the CLIs' local log files on
  your disk. The logs never leave your machine; only the public price
  tables are downloaded.

## The local HTTP API

`http://127.0.0.1:6736/v1/usage` exists so your own scripts and widgets
can read your usage. It is loopback-only (nothing on your network can
reach it), serves usage numbers only (never credentials or keys), and
sends **no CORS headers** — so websites you visit cannot read it through
your browser. Details: [local-http-api.md](local-http-api.md).

## Verifying all of this

OpenMeter is MIT-licensed and this repository is the entire codebase. Search
it: there is no analytics SDK import, and every `http` call site lives
in a provider module ([`src-tauri/src/providers/`](../src-tauri/src/providers/)),
the pricing engine ([`src-tauri/src/pricing.rs`](../src-tauri/src/pricing.rs)),
the updater registration ([`src-tauri/src/lib.rs`](../src-tauri/src/lib.rs)),
or the one-file statistics module ([`src-tauri/src/telemetry.rs`](../src-tauri/src/telemetry.rs)).
