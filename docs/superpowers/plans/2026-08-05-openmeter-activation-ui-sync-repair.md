# OpenMeter Activation, UI, and Sync Repair Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure Windows Search starts only the signed MSIX runtime, restore the complete dashboard without the left-edge sliver, and enroll the Mini PC as a second encrypted sync client.

**Architecture:** The MSIX installer will retire the exact legacy per-user installation and reject health checks served by any runtime outside the installed package. The dashboard keeps its current lazy WebView lifecycle, removes the intentional four-pixel sidebar peek, and resets the card viewport on every explicit open. The Mini PC remains the Tailscale hub and also receives the signed MSIX client, with its device and history credentials provisioned over SSH without printing secrets.

**Tech Stack:** PowerShell 5.1, MSIX/AppX, Tauri 2, Rust, TypeScript/Vitest, Windows Credential Manager, Tailscale, OpenSSH.

## Global Constraints

- Windows 11 only, for the laptop and Mini PC in this task.
- Keep all OpenMeter data outside OneDrive.
- Do not weaken Smart App Control or code-integrity policy.
- Keep enrollment tokens, recovery material, and device credentials out of command output and persistent plaintext files.
- Preserve the existing `%APPDATA%\OpenMeter` data root and all usage history.
- Leave the unavailable desktop untouched.

---

### Task 1: Retire the duplicate loose installation and strengthen MSIX acceptance

**Files:**
- Modify: `deployment/private-msix/PrivateMsix.psm1`
- Modify: `deployment/private-msix/install-private-msix.ps1`
- Modify: `scripts/test-private-msix.ps1`

**Interfaces:**
- Produces: `Disable-OpenMeterLegacyInstall -LegacyRoot <path> -BackupRoot <path> -StartMenuRoots <string[]> -RunKey <registry path>` returning the backup directory or `$null`.
- Produces: `Assert-OpenMeterRuntimeOrigin -InstallLocation <path> -Processes <object[]>` returning the expected packaged tray path or throwing on a legacy process.

- [ ] **Step 1: Write failing installer tests**

Add fixtures that create an exact legacy root, HKCU test Run value, and Start Menu shortcut. Assert that cleanup moves the root and shortcut into the backup, removes only the exact Run value, and rejects an unexpected target. Add fake process records proving acceptance rejects `C:\Users\Example\AppData\Local\OpenMeter\openmeter-tray.exe` and accepts `<InstallLocation>\VFS\ProgramFilesX64\OpenMeter\openmeter-tray.exe`.

- [ ] **Step 2: Run the failing tests**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\test-private-msix.ps1 -InstallOnly`

Expected: FAIL because `Disable-OpenMeterLegacyInstall` and `Assert-OpenMeterRuntimeOrigin` do not exist.

- [ ] **Step 3: Implement exact, recoverable legacy retirement**

Implement the two exported functions. Cleanup must resolve full paths, stop only matching `openmeter-tray.exe` processes, move matching shortcuts and the legacy root to a timestamped backup outside the active path, and refuse any path outside the supplied legacy root. After AppsFolder activation and HTTP 200, the installer must query live `openmeter-tray.exe` executable paths and call `Assert-OpenMeterRuntimeOrigin` before writing acceptance evidence.

- [ ] **Step 4: Run installer tests**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\test-private-msix.ps1`

Expected: PASS with no package registration or trust changes left by tests.

- [ ] **Step 5: Commit**

```powershell
git add deployment/private-msix/PrivateMsix.psm1 deployment/private-msix/install-private-msix.ps1 scripts/test-private-msix.ps1
git commit -m "fix(msix): retire duplicate OpenMeter runtime"
```

### Task 2: Restore dashboard entry state and remove the left-edge sliver

**Files:**
- Modify: `src/layout.ts`
- Modify: `tests/layout.test.ts`
- Modify: `src/main.ts`
- Modify: `src/styles.css`
- Modify: `index.html`

**Interfaces:**
- Produces: `resetDashboardViewport(viewport: { scrollTop: number }): void`.

- [ ] **Step 1: Write the failing viewport test**

```ts
import { resetDashboardViewport } from "../src/layout";

test("explicit popover entry returns the complete dashboard to its first card", () => {
  const viewport = { scrollTop: 640 };
  resetDashboardViewport(viewport);
  expect(viewport.scrollTop).toBe(0);
});
```

- [ ] **Step 2: Run the failing test**

Run: `npm test -- --run tests/layout.test.ts`

Expected: FAIL because `resetDashboardViewport` is not exported.

- [ ] **Step 3: Implement and wire the viewport reset**

Implement the helper as `viewport.scrollTop = 0` and call it from the existing `popover-shown` handler before trail activation and reveal animation.

- [ ] **Step 4: Remove the intentional sidebar peek**

Keep `#side-zone` as the invisible ten-pixel hover target, but change the sidebar resting transform from `translateX(calc(-100% + 4px))` to `translateX(-100%)`. Update the HTML/CSS comments so no visible sliver is part of the design.

- [ ] **Step 5: Run frontend tests and build**

Run: `npm test -- --run`

Run: `npm run build`

Expected: 18 existing tests plus the new viewport test pass, and Vite builds successfully.

- [ ] **Step 6: Commit**

```powershell
git add src/layout.ts tests/layout.test.ts src/main.ts src/styles.css index.html
git commit -m "fix(ui): restore complete dashboard on open"
```

### Task 3: Build, install, and prove the signed laptop runtime

**Files:**
- Generated: `artifacts/private-msix/OpenMeter_0.5.0.12_x64.msix`
- Generated: `artifacts/private-msix/acceptance.json`

**Interfaces:**
- Consumes: Task 1 installer acceptance and Task 2 UI bundle.
- Produces: one running signed laptop tray process under the MSIX install location.

- [ ] **Step 1: Build revision 12**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File deployment\private-msix\build-private-msix.ps1 -PackageRevision 12`

Expected: all three payload executables and the MSIX have valid signatures from the configured private deployment certificate.

- [ ] **Step 2: Install with legacy retirement**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File deployment\private-msix\install-private-msix.ps1`

Expected: the loose install is moved to a recoverable backup, the old Run value and shortcut disappear, and acceptance records the packaged runtime path.

- [ ] **Step 3: Verify cold and repeated Search activation**

Cold-start and repeat `shell:AppsFolder\OpenMeter.Private_hcan0ng7hsgfc!OpenMeter`; assert one process, executable path under the package install location, HTTP 200 at `127.0.0.1:6736/v1/limits`, and no visible dashboard window before an explicit tray click.

### Task 4: Enroll the Mini PC client and verify encrypted two-device tracking

**Files:**
- Remote install: signed OpenMeter MSIX on `MINIPC`
- Remote update: signed `C:\Program Files\OpenMeter Sync Hub\openmeter-sync-hub.exe`
- Remote data: `%APPDATA%\OpenMeter\config.json` and Windows Credential Manager targets

**Interfaces:**
- Consumes: laptop history key from `OpenMeter/sync/history-key` in Credential Manager and a one-use hub enrollment token.
- Produces: a distinct Mini PC device ID, matching history key, Mini PC device credential, and successful v1/v2 sync revisions on both devices.

- [ ] **Step 1: Back up and replace the hub binary**

Over SSH to `lucas@100.90.87.7`, stop `OpenMeterSyncHub`, retain the old executable as `.previous.exe`, extract the individually signed hub executable from the signed MSIX, restart the service, and require `/health` HTTP 204 plus a valid executable signature.

- [ ] **Step 2: Install the signed MSIX for the Mini PC user**

Copy the MSIX and public certificate over Tailscale, establish the same publisher trust, install for user `lucas`, and verify the package registration and data root outside OneDrive.

- [ ] **Step 3: Provision credentials without plaintext persistence**

Generate one enrollment token on the hub. In one in-memory local PowerShell pipeline, read the existing 32-byte laptop history key from Credential Manager, pass both values over SSH standard input, enroll the Mini PC, store the returned device credential and shared history key in the Mini PC Credential Manager, zero temporary byte arrays, and write only non-secret sync metadata to its config.

- [ ] **Step 4: Start both clients and synchronize**

Label the laptop `Laptop` and Mini PC `Mini PC`, start the packaged tray on both devices, and wait on `syncLastSuccess` and `syncTrackingLastSuccess` rather than sleeping a fixed duration.

- [ ] **Step 5: Verify end to end**

Require two active hub device IDs, current v2 envelopes from both labels, non-empty peer projections on both machines, unchanged local history outside OneDrive, hub HTTP 204, and laptop local API HTTP 200. Do not print tokens, credentials, or recovery material.

### Task 5: Final verification and branch handoff

**Files:**
- Verify only: committed source, signed artifact, installed laptop and Mini PC state.

- [ ] **Step 1: Run the full applicable suite**

Run frontend tests, production build, private MSIX tests, `git diff --check`, hub tests available under Smart App Control, package signature verification, and exact runtime-origin checks.

- [ ] **Step 2: Inspect final Git state**

Only intentional source changes may be committed. Existing `artifacts/` and `deployment/e9844e2/` remain untracked and untouched.

- [ ] **Step 3: Report evidence and the single visual check**

Report installed versions, process paths, API status, hub health, two-device state, test counts, commits, and ask the user only to click the tray icon once for final visual confirmation of the complete top-of-dashboard layout and absent left sliver.
