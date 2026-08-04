# Private MSIX Deployment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build, privately sign, install, and verify OpenMeter as an x64 MSIX on the owner's Windows 11 machines without Azure Artifact Signing or disabling application control.

**Architecture:** Focused PowerShell modules generate a deterministic package layout, manage one non-exportable local RSA signing identity, sign payloads and the MSIX, and install only after certificate/package verification. The public release workflow remains unchanged; the private deployment output stays under ignored local artifacts.

**Tech Stack:** PowerShell 7/Windows PowerShell 5.1, Rust/Cargo, Tauri 2, Windows SDK MakeAppx and SignTool, AppX/MSIX deployment cmdlets, Pester-free executable PowerShell tests.

## Global Constraints

- Windows 11 x64 only.
- Keep all OpenMeter state under `%APPDATA%\OpenMeter`, outside OneDrive.
- Never disable Smart App Control or App Control for Business.
- Use RSA/SHA-256 code signing and a non-exportable private key.
- Export only the public `.cer`; never export or transmit the private key.
- Do not change `.github/workflows/release.yml` or weaken public Authenticode checks.
- Fail closed on certificate, signature, package, launch, API, or sync verification errors.

---

### Task 1: Deterministic private package layout

**Files:**
- Create: `deployment/private-msix/PrivateMsix.psm1`
- Create: `deployment/private-msix/AppxManifest.template.xml`
- Create: `deployment/private-msix/README.md`
- Test: `scripts/test-private-msix.ps1`

**Interfaces:**
- Consumes: release binaries and icon assets produced by the existing Tauri/Cargo builds.
- Produces: `New-OpenMeterMsixLayout -InputRoot <path> -OutputRoot <path> -Version <a.b.c.d> -Publisher <subject>` and `Resolve-WindowsSdkTool -Name <MakeAppx.exe|SignTool.exe>`.

- [ ] **Step 1: Write the failing layout test**

Create a temporary fixture with three PE-named payload files and PNG assets. Import `PrivateMsix.psm1`, call `New-OpenMeterMsixLayout`, and assert literal destinations, manifest identity `OpenMeter.Private`, processor architecture `x64`, full-trust entry point `openmeter-tray.exe`, and absence of source/user paths in the manifest.

- [ ] **Step 2: Run the test and verify RED**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-private-msix.ps1 -LayoutOnly`

Expected: FAIL because `deployment/private-msix/PrivateMsix.psm1` does not exist.

- [ ] **Step 3: Implement the minimal layout module and template**

Implement strict path validation, four-part numeric version validation, XML escaping through `System.Xml`, deterministic copies to `VFS\ProgramFilesX64\OpenMeter`, and manifest generation from the template. `Resolve-WindowsSdkTool` searches installed Windows Kits `bin\<version>\x64` directories newest-first and returns one exact path or throws.

- [ ] **Step 4: Run the layout test and verify GREEN**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-private-msix.ps1 -LayoutOnly`

Expected: PASS with temporary files removed.

- [ ] **Step 5: Commit Task 1**

Run: `git add deployment/private-msix scripts/test-private-msix.ps1 && git commit -m "feat: stage deterministic private MSIX layout"`

### Task 2: Private certificate, payload signing, and package build

**Files:**
- Modify: `deployment/private-msix/PrivateMsix.psm1`
- Create: `deployment/private-msix/build-private-msix.ps1`
- Modify: `scripts/test-private-msix.ps1`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: `New-OpenMeterMsixLayout`, Windows SDK tools, existing `npm`/Cargo projects.
- Produces: `Get-OrCreateOpenMeterSigningCertificate -Subject <subject>`, `Assert-OpenMeterSignature -Path <path> -Thumbprint <hex>`, and local outputs `artifacts/private-msix/OpenMeter_<version>_x64.msix`, `OpenMeter-Private.cer`, `SHA256SUMS.txt`.

- [ ] **Step 1: Write failing certificate and signature tests**

Use a unique test subject, assert an RSA 3072-bit Code Signing certificate is created in `Cert:\CurrentUser\My`, is non-exportable, is reused on the second call, and that `Assert-OpenMeterSignature` rejects an unsigned fixture. Clean up only the unique test certificate in `finally`.

- [ ] **Step 2: Run the focused test and verify RED**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-private-msix.ps1 -SigningOnly`

Expected: FAIL because the signing functions are not exported.

- [ ] **Step 3: Implement signing identity and build orchestration**

Create the certificate with `New-SelfSignedCertificate -Type CodeSigningCert -KeyAlgorithm RSA -KeyLength 3072 -KeyExportPolicy NonExportable`. Build the frontend, tray, CLI, and hub; stage them; call SignTool with `/fd SHA256`; call MakeAppx with `/o`; sign the package; export only `.cer`; and write checksums. Reject ambiguous certificates, stale/expired certificates, wrong subjects, missing SDK tools, or nonzero child-process exit codes.

- [ ] **Step 4: Run signing tests and verify GREEN**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-private-msix.ps1 -SigningOnly`

Expected: PASS and the temporary test certificate is absent afterward.

- [ ] **Step 5: Run the complete script test**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-private-msix.ps1`

Expected: PASS for layout validation, certificate lifecycle, rejection behavior, and parsing.

- [ ] **Step 6: Commit Task 2**

Run: `git add .gitignore deployment/private-msix scripts/test-private-msix.ps1 && git commit -m "feat: build privately signed OpenMeter MSIX"`

### Task 3: Verified installation and laptop acceptance

**Files:**
- Modify: `deployment/private-msix/PrivateMsix.psm1`
- Create: `deployment/private-msix/install-private-msix.ps1`
- Create: `deployment/private-msix/uninstall-private-msix.ps1`
- Modify: `scripts/test-private-msix.ps1`
- Create: `artifacts/private-msix/acceptance.json` (ignored local output)

**Interfaces:**
- Consumes: signed MSIX and public certificate from Task 2.
- Produces: `Install-OpenMeterPrivateMsix -PackagePath <path> -CertificatePath <path> -ExpectedThumbprint <hex>` and machine-readable acceptance evidence.

- [ ] **Step 1: Write failing installer validation tests**

Assert the installer rejects a wrong thumbprint before changing trust, rejects an invalid package signature, and requires the manifest publisher to equal the certificate subject. Exercise validation with temporary fixtures and verify no test certificate remains trusted.

- [ ] **Step 2: Run installer tests and verify RED**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-private-msix.ps1 -InstallOnly`

Expected: FAIL because installation functions do not exist.

- [ ] **Step 3: Implement verified install and explicit uninstall**

Validate SHA-256/thumbprint and package signature, import the public certificate into `Cert:\CurrentUser\TrustedPeople`, call `Add-AppxPackage`, launch the registered app, and poll `http://127.0.0.1:6736/v1/limits` for HTTP 200. The uninstall script removes `OpenMeter.Private` for the current user; `-RemoveCertificate` is an explicit separate switch.

- [ ] **Step 4: Run installer tests and verify GREEN**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-private-msix.ps1 -InstallOnly`

Expected: PASS without changing persistent trust.

- [ ] **Step 5: Build the real package**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File deployment/private-msix/build-private-msix.ps1`

Expected: exit 0; one x64 MSIX, one CER, and checksums; all PE and package signatures match the exported certificate.

- [ ] **Step 6: Install and verify on this laptop**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File deployment/private-msix/install-private-msix.ps1 -PackagePath <generated-msix> -CertificatePath <generated-cer>`

Expected: AppX registration succeeds without changing application-control policy; tray launches; API returns HTTP 200; `%APPDATA%\OpenMeter` remains outside OneDrive; Mini PC sync hub responds and v2 success metadata advances.

- [ ] **Step 7: Verify full project health**

Run: `npm test -- --run; npm run build; cargo test --manifest-path src-tauri/Cargo.toml; powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-private-msix.ps1; git diff --check`

Expected: every command exits 0 with no test failures.

- [ ] **Step 8: Commit Task 3**

Run: `git add deployment/private-msix scripts/test-private-msix.ps1 && git commit -m "feat: verify private MSIX installation"`

