# Trusted Windows releases

OpenMeter releases are built on GitHub-hosted Windows runners. Publishing fails closed unless the tray, CLI, sync hub, and final NSIS installer all have a valid timestamped Microsoft Trusted Signing signature from the configured publisher. Tauri updater signatures, SHA-256 checksums, and GitHub provenance attestations remain separate mandatory layers.

## One-time owner setup

1. Confirm that the owner's legal identity and geography are supported by Microsoft Trusted Signing.
2. Create an Azure Artifact Signing account, complete identity validation, and create an RSA Public Trust certificate profile.
3. Grant the workload identity the **Artifact Signing Certificate Profile Signer** role on that profile.
4. Create a Microsoft Entra application/service principal and a GitHub OIDC federated credential restricted to `aGamingGod1234/openmeter` and the protected release environment or tag subject used by this repository. Do not create a client secret.
5. Configure these GitHub repository variables: `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID`, `ARTIFACT_SIGNING_ENDPOINT`, `ARTIFACT_SIGNING_ACCOUNT`, `ARTIFACT_SIGNING_PROFILE`, and `AUTHENTICODE_PUBLISHER`.
6. Keep `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as repository secrets for updater signatures.

The workflow intentionally fails during preflight when any variable is absent. The current repository is not Authenticode-ready until the Azure account, identity validation, role assignment, federated credential, and variables above exist.

## Release procedure

Run the Release workflow manually first. Verify all Authenticode checks and inspect the uploaded dry-run artifact. Then bump `src-tauri/tauri.conf.json`, push a matching `v<version>` tag, and require the tag workflow to pass before using the release. Every release body declares its exact Authenticode publisher for `install.ps1` to verify.
