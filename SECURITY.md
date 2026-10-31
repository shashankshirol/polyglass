# Security policy

Polyglass captures your screen, so we take security reports seriously.

## Reporting a vulnerability

Please **do not** open a public issue. Report privately via GitHub Security Advisories
("Report a vulnerability" on the repository's Security tab) once the repository is public, or
e-mail **security@polyglass.invalid** (placeholder, see CHECKLIST M0-REPO-6).

Include steps to reproduce, affected versions/platforms, and impact. We aim to acknowledge
within 3 working days and to publish a fix and advisory within 90 days.

## Scope

In scope: anything that could leak screen content or text off-device, bypass model integrity
checks (SHA-256, manifest-only URLs), weaken TLS, run untrusted code in the webview, or expose
API keys.

## Supported versions

Before 1.0, only the latest release is supported.
