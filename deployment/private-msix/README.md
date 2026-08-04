# Private MSIX deployment

This directory contains the owner-only Windows 11 packaging path described in
`docs/superpowers/specs/2026-08-04-private-msix-deployment-design.md`.

It is deliberately separate from the public release workflow. A package made
here is trusted only after its public certificate is explicitly installed in
`LocalMachine\TrustedPeople` on an owned device. The installer uses a small
elevated helper and Windows UAC for that one machine-wide trust change; package
registration and data remain per-user. Never publish the private deployment
artifacts as a public OpenMeter release.

The tray application is allowed through MSIX packaged activation on the target
PC. Its Enterprise Code Integrity policy still rejects direct execution of the
native `openmeter.exe` CLI with event 3077 because the private certificate is
not publicly trusted. The private package therefore does not register a CLI
execution alias. The dashboard and read-only local HTTP API remain available;
public signing is still required for the native CLI.
