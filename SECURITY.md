# Security policy

## Reporting a vulnerability

Please report security issues privately, not in a public issue:

- **GitHub:** use "Report a vulnerability" under this repository's Security tab
  (private vulnerability reporting).
- **Email:** hello@getnodea.com, with "Security" in the subject.

Include what you found, how to reproduce it, and the app version and platform.
We will acknowledge your report, keep you updated while we fix it, and credit
you in the release notes unless you would rather not be named.

## Scope

This repository is the Nodea desktop app. In scope:

- The app itself (the Rust code in `src-tauri/` and the UI in `src/`).
- How it stores credentials and data on the user's machine.
- What it accepts from the Nodea backend over gRPC. The app treats the backend
  as untrusted for anything that touches the local filesystem or system.

The Nodea backend and web console are closed source. Report issues in them to
the same addresses.

## Supported versions

Only the latest release receives fixes.
