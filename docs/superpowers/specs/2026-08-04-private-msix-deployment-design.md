# Private MSIX Deployment Design

## Goal

Install the current OpenMeter Windows build on the owner's Windows 11 laptop, desktop, and Mini PC without Azure Artifact Signing and without disabling Smart App Control or App Control for Business.

## Trust boundary

The private deployment channel uses an RSA code-signing certificate created for `CN=OpenMeter Private Deployment`. The private key remains non-exportable in the laptop user's Windows certificate store. Only the public certificate is copied to another owned device. Each device must explicitly trust that public certificate before installing the package.

This certificate is only for the owner's machines. It is not publicly trusted, must not be used by the public GitHub release workflow, and does not make downloads safe for general distribution. The existing fail-closed Azure/public release workflow remains unchanged.

## Packaging architecture

A PowerShell build script will:

1. Build the OpenMeter frontend and the release versions of `openmeter-tray.exe`, `openmeter.exe`, and `openmeter-sync-hub.exe` from this checkout.
2. Stage a deterministic MSIX layout under a temporary local directory outside OneDrive.
3. Sign every executable payload with the private certificate and a SHA-256 digest.
4. Generate an Appx manifest for an unpackaged full-trust desktop application, with OpenMeter's existing icon assets and per-user data paths unchanged.
5. Create and sign an x64 MSIX package.
6. Export only the public `.cer` file alongside the package and write SHA-256 checksums.

The package launches the tray executable. The CLI and sync-hub executable remain signed packaged resources. The target Enterprise Code Integrity policy permits the tray through packaged activation but rejects direct native CLI execution with event 3077 until the publisher is publicly trusted, so this private package does not expose a broken CLI alias. Hub service installation stays an explicit administrator operation performed by the existing hub installer; MSIX installation will not silently create a service.

## Installation and removal

A separate installer script verifies the package signature and expected certificate thumbprint, uses an administrator-approved helper to import the public certificate into `LocalMachine\TrustedPeople` (the narrowest store accepted by the Windows AppX deployment service for this self-signed publisher), installs the package for the current user, launches OpenMeter, and verifies the local API. It fails closed on a signature mismatch, an untrusted package, a blocked executable, or a failed health check.

Removal uses normal Windows package removal. The public certificate is removed only through a separate explicit cleanup switch so uninstalling OpenMeter does not unexpectedly alter trust state used by another installed version.

## Verification

Automated tests will exercise the scripts against temporary certificate stores and staged fixtures before implementation is accepted. Live laptop acceptance requires all of the following:

- MSIX signature and package integrity validate.
- Windows installs it without disabling or bypassing application control.
- `openmeter-tray.exe` starts and renders the dashboard.
- `http://127.0.0.1:6736/v1/limits` returns HTTP 200.
- Existing `%APPDATA%\OpenMeter` state remains outside OneDrive and is preserved.
- Cross-device sync reaches the Mini PC hub and records a successful v2 sync.
- Uninstall removes the package while preserving user data.

If Windows policy rejects the explicitly trusted private certificate, the scripts stop and report that evidence. They will not add App Control exceptions or disable security features. In that case, the remaining safe routes are a publicly trusted certificate or Microsoft Store signing.
