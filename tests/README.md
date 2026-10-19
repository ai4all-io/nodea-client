# Integration Tests

Playwright tests for the Nodea desktop app's UI, run against the Vite dev
server in a real browser.

## Prerequisites

None. There is no database to seed, no Supabase project to reach and no test
user to create — `npm test` works on a fresh clone.

The suite gets there two ways:

- **Demo mode** (`?demo=true`). `AuthContext` authenticates a demo user
  locally, `src/lib/api.ts` swaps in `src/lib/mockApi.ts` instead of the real
  REST client, and `src/lib/mockTauri.ts` answers the `invoke` calls the Tauri
  host would normally serve. `?phase=<name>` pins the node lifecycle to one
  state instead of waiting through the timed simulation.
- **Route stubs**, for the handful of cases demo mode cannot express — notably
  a *failed* login, since demo mode's `login()` always succeeds.

## Running Tests

```bash
npm ci
npx playwright install --with-deps chromium

npm test                      # all specs
npm test tests/login.spec.ts  # one spec
npm run test:headed           # watch the browser
npm run test:ui               # Playwright's UI mode
npm run test:report           # open the last HTML report
```

Playwright starts the dev server itself (`webServer` in
`playwright.config.ts`), loading `.env.test`.

## Test Coverage

| Spec | Covers |
|---|---|
| `node.spec.ts` | Node page and sidebar status widget across every lifecycle phase, and the sharing toggle |
| `fleet.spec.ts` | Fleet page listing and device detail |
| `login.spec.ts` | Login form rendering, the failure path, a successful sign-in redirect, the signup link and the Google button |
| `settings.spec.ts` | Settings rows, the absence of controls with no backend support, app version and sign out |

## What this does not cover

Nothing here exercises `src/lib/api.ts` against a live backend — the Playwright
suite deliberately has no backend dependency, and the vitest-style mocks it
uses cannot catch request/response drift. If you change `src/lib/api.ts`, check
it against the backend's `/site` API by hand (the backend is closed source).

## Troubleshooting

**A spec hangs on a selector.** Tests wait for `domcontentloaded` plus specific
elements, never `networkidle`: the sidebar keeps a persistent device-status
`EventSource` open on every authenticated page, so the network never truly goes
idle while signed in.

**Demo mode leaking between tests.** `isDemoMode()` caches its answer in
`localStorage`, so a spec that visits `?demo=true` would affect later
navigations in the *same* browser context. Playwright gives each test a fresh
context, so this only matters if you add `page.goto` calls across a shared
context yourself — pass `?demo=false` to clear it explicitly.
