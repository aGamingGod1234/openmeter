# Private MSIX deployment

This directory contains the owner-only Windows 11 packaging path described in
`docs/superpowers/specs/2026-08-04-private-msix-deployment-design.md`.

It is deliberately separate from the public release workflow. A package made
here is trusted only after its public certificate is explicitly installed on
an owned device. Never publish the private deployment artifacts as a public
OpenMeter release.

