# Contributing

Thanks for your interest in Nodea Desktop. Bug reports and pull requests are
welcome.

## Before you start

The app needs the Nodea backend to do anything useful, and that backend is
closed source. You can still build the app, run the UI in demo mode, and run
both test suites without it. See the Quick Start in [README.md](README.md) and
[tests/README.md](tests/README.md).

For anything bigger than a small fix, please open an issue first so we can
agree on the approach.

## Making a change

1. Fork the repository and create a branch from `main`.
2. Make the change, with tests where they apply.
3. Run the checks CI runs:

   ```bash
   cp .env.test .env.local
   (cd src-tauri && cargo test --all-targets)
   npx tsc --noEmit && npm run build
   npm test                    # Playwright, no backend needed
   ```

   If you changed dependencies, regenerate the licence notices with
   `make licenses` (see the Third-party licences section of the README).
4. Open a pull request describing what changed and why.

Security issues go through [SECURITY.md](SECURITY.md), not a pull request.

## Developer Certificate of Origin

Contributions are accepted under the [Developer Certificate of Origin
1.1](https://developercertificate.org/). By signing off a commit, you certify
that you wrote it or otherwise have the right to submit it under the project's
licence. Add the sign-off with `git commit -s`, which appends:

```
Signed-off-by: Your Name <your.email@example.com>
```

Every commit in a pull request needs one. A check on pull requests enforces it.

## Licence

Nodea Desktop is licensed under the [Apache License 2.0](LICENSE). Contributions
are made under the same licence (inbound = outbound), as section 5 of the
licence describes.
