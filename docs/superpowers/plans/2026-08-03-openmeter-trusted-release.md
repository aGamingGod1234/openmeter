# OpenMeter Trusted Windows Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce, publish, install, update, and live-verify an OpenMeter release whose inner executables and NSIS installer carry valid timestamped Authenticode signatures.

**Architecture:** GitHub Actions builds unsigned release binaries, authenticates to Azure with OIDC, signs inner PE files through Azure Artifact Signing, bundles those exact files with Tauri, signs the finished NSIS installer, and fails closed unless every signature and updater artifact validates. The installer script independently verifies the published hash and Authenticode chain before execution.

**Tech Stack:** GitHub Actions, Azure Login v3, Azure Artifact Signing Action v2, OIDC workload identity, Tauri CLI 2, NSIS, PowerShell 5.1+, Authenticode, SHA-256, Tauri updater signatures.

## Global Constraints

- Use RSA public-trust Authenticode with SHA-256 and RFC3161 timestamping at `http://timestamp.acs.microsoft.com`.
- Use `azure/artifact-signing-action@v2` and `azure/login@v3`; never store an Azure client secret when OIDC is available.
- Sign `openmeter-tray.exe`, `openmeter.exe`, and `openmeter-sync-hub.exe` before bundling.
- Sign and verify the finished NSIS installer after bundling.
- Keep Tauri updater signatures as a separate mandatory verification layer.
- A release workflow without signing configuration must fail before publication; it must not silently publish unsigned files.
- Installation remains per-user under `%LOCALAPPDATA%\OpenMeter`; data remains under `%APPDATA%\OpenMeter`; no OneDrive.
- Never disable Smart App Control or install a private root certificate.
- Azure Artifact Signing public-trust eligibility must be confirmed before account creation: organizations are currently limited to the USA, Canada, EU, and UK; individuals to the USA and Canada.
- This PC's final acceptance requires Smart App Control to allow the signed tray executable.

---

## File map

- `.github/workflows/release.yml`: OIDC login, ordered inner signing, bundling, installer signing, verification, attestation, publication.
- `.github/workflows/ci.yml`: release-script invariant tests and unsigned dry-build coverage.
- `scripts/test-release-workflow.ps1`: static ordering and fail-closed checks.
- `scripts/test-authenticode.ps1`: signature, chain, timestamp, publisher, and payload verification.
- `scripts/prepare-bundle.ps1`: stages already-signed inner binaries without mutation.
- `scripts/test-prepare-bundle.ps1`: proves hashes are preserved while staging.
- `install.ps1`: verifies hash and Authenticode before launching NSIS.
- `src-tauri/tauri.bundle.conf.json`: exact signed resources included in NSIS.
- `docs/releasing.md`: owner setup and repeatable release procedure.

### Task 1: Release workflow invariants and signing preflight

**Files:**
- Create: `scripts/test-release-workflow.ps1`
- Create: `scripts/test-authenticode.ps1`
- Modify: `.github/workflows/ci.yml`
- Create: `docs/releasing.md`

**Interfaces:**
- Produces: `test-release-workflow.ps1` static policy check.
- Produces: `test-authenticode.ps1 -Path <file> -ExpectedPublisher <subject>` fail-closed verifier.
- Consumed by every later task.

- [ ] **Step 1: Write the failing workflow policy test**

```powershell
$workflow = Get-Content "$PSScriptRoot\..\.github\workflows\release.yml" -Raw
$required = @(
  'azure/login@v3',
  'azure/artifact-signing-action@v2',
  'timestamp.acs.microsoft.com',
  'Test-AuthenticodeSignature',
  'tauri.cmd bundle',
  'tauri.cmd signer sign'
)
$missing = $required | Where-Object { $workflow -notmatch [regex]::Escape($_) }
if ($missing) { throw "Release workflow misses: $($missing -join ', ')" }
$inner = $workflow.IndexOf('Sign inner executables')
$bundle = $workflow.IndexOf('Bundle NSIS')
$installer = $workflow.IndexOf('Sign NSIS installer')
if (-not (0 -le $inner -and $inner -lt $bundle -and $bundle -lt $installer)) {
  throw 'Signing order must be inner binaries, bundle, installer'
}
```

- [ ] **Step 2: Run and verify RED**

Run: `powershell -NoProfile -File scripts/test-release-workflow.ps1`

Expected: failure because the current release workflow has no Artifact Signing steps.

- [ ] **Step 3: Implement Authenticode verifier and document owner preflight**

Verifier core:

```powershell
$signature = Get-AuthenticodeSignature -LiteralPath $Path
if ($signature.Status -ne 'Valid') { throw "Invalid Authenticode signature: $($signature.StatusMessage)" }
if ($signature.SignerCertificate.Subject -notlike "*$ExpectedPublisher*") { throw 'Unexpected publisher subject' }
if (-not $signature.TimeStamperCertificate) { throw 'Authenticode signature has no timestamp' }
if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -notmatch '^[0-9A-F]{64}$') { throw 'SHA-256 unavailable' }
```

Document the exact owner preflight: verify supported legal identity/geography, create an Azure Artifact Signing account, complete identity validation, create a Public Trust certificate profile using RSA, assign `Artifact Signing Certificate Profile Signer`, create a GitHub OIDC federated credential restricted to repository `aGamingGod1234/openmeter` and the release environment, then set repository variables `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID`, `ARTIFACT_SIGNING_ENDPOINT`, `ARTIFACT_SIGNING_ACCOUNT`, `ARTIFACT_SIGNING_PROFILE`, and `AUTHENTICODE_PUBLISHER`.

- [ ] **Step 4: Run static policy and PowerShell parser checks**

Run: `powershell -NoProfile -Command "[scriptblock]::Create((Get-Content scripts/test-authenticode.ps1 -Raw)) | Out-Null"`

Run: `powershell -NoProfile -File scripts/test-release-workflow.ps1`

Expected: parser passes; policy remains RED until Task 2 wires the action.

- [ ] **Step 5: Commit**

```powershell
git add scripts/test-release-workflow.ps1 scripts/test-authenticode.ps1 .github/workflows/ci.yml docs/releasing.md
git commit -m "test: define trusted release invariants"
```

### Task 2: OIDC login and signed inner executables

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/test-release-workflow.ps1`

**Interfaces:**
- Consumes: approved Azure Artifact Signing account/profile and repository variables from Task 1.
- Produces: valid signed `openmeter-tray.exe`, `openmeter.exe`, and `openmeter-sync-hub.exe` before bundling.

- [ ] **Step 1: Extend the failing test to require fail-closed variable checks**

```powershell
foreach ($name in 'AZURE_CLIENT_ID','AZURE_TENANT_ID','AZURE_SUBSCRIPTION_ID','ARTIFACT_SIGNING_ENDPOINT','ARTIFACT_SIGNING_ACCOUNT','ARTIFACT_SIGNING_PROFILE','AUTHENTICODE_PUBLISHER') {
  if ($workflow -notmatch $name) { throw "Missing release variable $name" }
}
if ($workflow -notmatch 'id-token:\s*write') { throw 'OIDC permission missing' }
```

- [ ] **Step 2: Run and verify RED**

Run: `powershell -NoProfile -File scripts/test-release-workflow.ps1`

Expected: failure listing missing Azure variables/login/action.

- [ ] **Step 3: Split Tauri compilation from bundling and sign inner files**

Add release steps in this order:

```yaml
- name: Build release binaries without bundling
  working-directory: src-tauri
  run: ..\node_modules\.bin\tauri.cmd build --no-bundle

- name: Build sync hub
  run: cargo build --manifest-path sync-hub/Cargo.toml --release

- name: Azure login
  uses: azure/login@v3
  with:
    client-id: ${{ vars.AZURE_CLIENT_ID }}
    tenant-id: ${{ vars.AZURE_TENANT_ID }}
    subscription-id: ${{ vars.AZURE_SUBSCRIPTION_ID }}

- name: Sign inner executables
  uses: azure/artifact-signing-action@v2
  with:
    endpoint: ${{ vars.ARTIFACT_SIGNING_ENDPOINT }}
    signing-account-name: ${{ vars.ARTIFACT_SIGNING_ACCOUNT }}
    certificate-profile-name: ${{ vars.ARTIFACT_SIGNING_PROFILE }}
    files: |
      ${{ github.workspace }}\src-tauri\target\release\openmeter-tray.exe
      ${{ github.workspace }}\src-tauri\target\release\openmeter.exe
      ${{ github.workspace }}\sync-hub\target\release\openmeter-sync-hub.exe
    file-digest: SHA256
    timestamp-rfc3161: http://timestamp.acs.microsoft.com
    timestamp-digest: SHA256
    description: OpenMeter
    description-url: https://github.com/aGamingGod1234/openmeter
```

Preflight variables with a PowerShell step that throws on any empty value before Azure login. Verify all three files immediately with `scripts/test-authenticode.ps1`.

- [ ] **Step 4: Run static test and an authorized workflow dry run**

Run: `powershell -NoProfile -File scripts/test-release-workflow.ps1`

Run after Azure setup: `gh workflow run release.yml --ref main` and watch the run to completion.

Expected: inner signing and signature verification pass; no release is created during `workflow_dispatch`.

- [ ] **Step 5: Commit**

```powershell
git add .github/workflows/release.yml scripts/test-release-workflow.ps1
git commit -m "ci: sign OpenMeter release executables with Artifact Signing"
```

### Task 3: Hash-preserving staging and NSIS bundling

**Files:**
- Modify: `scripts/prepare-bundle.ps1`
- Modify: `scripts/test-prepare-bundle.ps1`
- Modify: `src-tauri/tauri.bundle.conf.json`
- Modify: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: signed inner executables from Task 2.
- Produces: NSIS installer containing byte-identical signed tray and CLI executables.

- [ ] **Step 1: Add a failing hash-preservation test**

```powershell
$before = (Get-FileHash $fixtureExe -Algorithm SHA256).Hash
& "$PSScriptRoot\prepare-bundle.ps1" -RepositoryRoot $tempRoot
$after = (Get-FileHash "$tempRoot\bundle-inputs\openmeter.exe" -Algorithm SHA256).Hash
if ($before -ne $after) { throw 'Bundle staging mutated the signed CLI' }
```

Also assert `tauri.bundle.conf.json` maps `bundle-inputs/openmeter.exe` and that `mainBinaryName` remains `openmeter-tray`.

- [ ] **Step 2: Run and verify RED**

Run: `powershell -NoProfile -File scripts/test-prepare-bundle.ps1`

Expected: failure until the fixture covers both signed PE resources and hash preservation.

- [ ] **Step 3: Stage signed files and call the bundle-only command**

After inner verification, run:

```yaml
- name: Stage signed bundle inputs
  shell: pwsh
  run: .\scripts\prepare-bundle.ps1

- name: Bundle NSIS
  working-directory: src-tauri
  run: ..\node_modules\.bin\tauri.cmd bundle --no-sign --bundles nsis --config tauri.bundle.conf.json
```

Extract the installer payload with the already-verified 7-Zip workflow and compare the embedded `openmeter-tray.exe` and `resources\openmeter.exe` Authenticode status/publisher with their pre-bundle inputs.

- [ ] **Step 4: Run bundle tests and dry release workflow**

Run: `powershell -NoProfile -File scripts/test-prepare-bundle.ps1`

Run: `gh workflow run release.yml --ref main`.

Expected: bundle test passes and extracted inner files retain valid signatures.

- [ ] **Step 5: Commit**

```powershell
git add scripts/prepare-bundle.ps1 scripts/test-prepare-bundle.ps1 src-tauri/tauri.bundle.conf.json .github/workflows/release.yml
git commit -m "ci: preserve signed binaries in NSIS bundle"
```

### Task 4: Sign, verify, hash, attest, and publish the installer

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/test-release-workflow.ps1`
- Modify: `install.ps1`
- Create: `scripts/test-install-script.ps1`

**Interfaces:**
- Consumes: unsigned NSIS package containing signed inner files.
- Produces: timestamped Authenticode-signed installer, Tauri updater signature, `latest.json`, `SHA256SUMS.txt`, and provenance attestation.

- [ ] **Step 1: Write failing installer verification tests**

```powershell
$installerScript = Get-Content "$PSScriptRoot\..\install.ps1" -Raw
foreach ($required in 'Get-AuthenticodeSignature','Status -ne','SignerCertificate','TimeStamperCertificate') {
  if ($installerScript -notmatch $required) { throw "install.ps1 misses $required" }
}
```

Add workflow order assertions: installer signing precedes `Generate latest.json`, which precedes attestation and upload/release.

- [ ] **Step 2: Run and verify RED**

Run: `powershell -NoProfile -File scripts/test-install-script.ps1`

Expected: failure because `install.ps1` currently verifies only SHA-256.

- [ ] **Step 3: Sign installer and fail closed in installer script**

Add the second Artifact Signing step targeting only `src-tauri\target\release\bundle\nsis\*-setup.exe`, using the same profile, SHA-256, and timestamp server. Verify it before generating manifests.

In `install.ps1`, after SHA-256 validation and before `Start-Process`, require:

```powershell
$signature = Get-AuthenticodeSignature -LiteralPath $dest
if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate) {
    Remove-Item $dest -Force -ErrorAction SilentlyContinue
    throw 'Installer Authenticode signature is invalid or not timestamped - not installing.'
}
$expectedPublisher = $release.body | Select-String -Pattern 'Authenticode publisher: `([^`]+)`'
if (-not $expectedPublisher) { throw 'Release does not declare its Authenticode publisher.' }
if ($signature.SignerCertificate.Subject -notlike "*$($expectedPublisher.Matches[0].Groups[1].Value)*") {
    throw 'Installer publisher does not match the release declaration.'
}
```

Generate the updater signature after final installer signing so `.sig`,
`sha256`, and attestation all describe the published bytes:

```yaml
- name: Sign final installer for Tauri updater
  working-directory: src-tauri
  shell: pwsh
  run: |
    $installer = Get-ChildItem target/release/bundle/nsis/*-setup.exe | Select-Object -First 1
    ..\node_modules\.bin\tauri.cmd signer sign $installer.FullName
  env:
    TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
```

When creating the GitHub release, append the repository variable as this exact
line so `install.ps1` has a publisher declaration to compare:

```text
Authenticode publisher: `${{ vars.AUTHENTICODE_PUBLISHER }}`
```

- [ ] **Step 4: Run tests and dry release**

Run: `powershell -NoProfile -File scripts/test-install-script.ps1`

Run: `powershell -NoProfile -File scripts/test-release-workflow.ps1`

Run: `gh workflow run release.yml --ref main`.

Expected: artifact includes a valid timestamped installer, matching `.sig`, matching SHA-256, and provenance.

- [ ] **Step 5: Commit**

```powershell
git add .github/workflows/release.yml install.ps1 scripts/test-install-script.ps1 scripts/test-release-workflow.ps1
git commit -m "ci: sign and verify the published OpenMeter installer"
```

### Task 5: Stable/beta manifests and signed update acceptance

**Files:**
- Modify: `.github/workflows/release.yml`
- Create: `scripts/new-update-manifests.ps1`
- Create: `scripts/test-update-manifests.ps1`
- Modify: `docs/releasing.md`

**Interfaces:**
- Consumes: signed installer and Tauri updater signature.
- Produces: `latest.json` for stable and `beta/latest.json` for prerelease-inclusive clients.

- [ ] **Step 1: Write failing manifest-selection tests**

```powershell
$stable = & "$PSScriptRoot\new-update-manifests.ps1" -Releases $fixtures -Channel stable
$beta = & "$PSScriptRoot\new-update-manifests.ps1" -Releases $fixtures -Channel beta
if ($stable.version -ne '1.2.0') { throw 'Stable selected prerelease' }
if ($beta.version -ne '1.3.0-beta.2') { throw 'Beta did not select newest semantic version' }
if (-not $stable.sha256 -or -not $stable.platforms.'windows-x86_64'.signature) { throw 'Manifest lacks integrity fields' }
```

- [ ] **Step 2: Run and verify RED**

Run: `powershell -NoProfile -File scripts/test-update-manifests.ps1`

Expected: generator does not exist.

- [ ] **Step 3: Implement deterministic channel manifests**

Stable excludes GitHub prereleases. Beta considers stable and prerelease tags and selects the highest SemVer. Both point to the final Authenticode-signed installer bytes and contain the Tauri signature plus project SHA-256. Release workflow uploads stable manifest to the normal latest-release asset and beta manifest under the `beta` release/tag used by the client endpoint.

- [ ] **Step 4: Run manifest tests and updater dry run**

Run: `powershell -NoProfile -File scripts/test-update-manifests.ps1`

Run the dry release workflow for a stable fixture and a beta fixture.

Expected: channel selection and integrity fields match fixtures.

- [ ] **Step 5: Commit**

```powershell
git add .github/workflows/release.yml scripts/new-update-manifests.ps1 scripts/test-update-manifests.ps1 docs/releasing.md
git commit -m "feat: publish signed stable and beta update channels"
```

### Task 6: Live acceptance for signed install, update, and uninstall

**Files:**
- Modify: `scripts/smoke-install.ps1`
- Create: `scripts/live-acceptance.ps1`
- Modify: `README.md`
- Modify: `SECURITY.md`

**Interfaces:**
- Consumes: published signed release and running Mini PC sync hub.
- Produces: machine-readable acceptance transcript under `outputs/acceptance/<version>/results.json` without secrets.

- [ ] **Step 1: Extend smoke test with failing signature and payload checks**

```powershell
Assert-ValidSignature $Installer $ExpectedPublisher
Install-Silently $Installer
Assert-ValidSignature "$env:LOCALAPPDATA\OpenMeter\openmeter-tray.exe" $ExpectedPublisher
Assert-ValidSignature "$env:LOCALAPPDATA\OpenMeter\resources\openmeter.exe" $ExpectedPublisher
```

- [ ] **Step 2: Run against the current unsigned artifact and verify RED**

Run: `powershell -NoProfile -File scripts/live-acceptance.ps1 -Installer <current-unsigned-installer>`

Expected: stops at invalid Authenticode signature; does not install.

- [ ] **Step 3: Publish a signed release and execute the complete acceptance flow**

The script verifies signature/hash, installs per-user, starts the tray, waits for the single-instance process, checks `openmeter --help`, queries `/v1/limits`, toggles privacy through the app command path, runs a sync health/encrypted round-trip, exercises stable/beta update selection, installs a newer signed build, verifies process restart, uninstalls, confirms PATH/autostart removal, and confirms `%APPDATA%\OpenMeter` remains.

- [ ] **Step 4: Confirm Smart App Control and acceptance transcript**

Run the signed installer normally on this PC without bypass commands or policy changes. Expected: installer and tray launch are not blocked by Smart App Control; `results.json` reports every acceptance item true and contains no token-like or user-path strings.

- [ ] **Step 5: Commit final verification documentation**

```powershell
git add scripts/smoke-install.ps1 scripts/live-acceptance.ps1 README.md SECURITY.md
git commit -m "test: verify signed Windows install and update lifecycle"
```
