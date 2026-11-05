# Chain Siren dashboard

A local Next.js / React interface for the Rust monitor, built with pnpm, Motion,
Highcharts, and TanStack Query. The Rust service remains the source of truth.

## Start the UI

From the repository root:

```sh
cd web
pnpm install --frozen-lockfile
pnpm dev
```

Open **http://127.0.0.1:3000**. Both dev and production commands bind to loopback.
The dashboard opens in a clearly labeled synthetic demo environment. Demo fixtures
stay in the browser and never enter the Rust database.

## Connect live data

Run the Rust service in another terminal, using your existing database and RPC
configuration:

```sh
# From the repository root; load your own .env only.
set -a
source .env
set +a
cargo run --release -- --config config/local.toml run
```

Generate a key in a separate terminal with the same environment loaded:

```sh
cargo run --release -- --config config/local.toml api-key --name dashboard
```

Choose **Connect live** in the UI and enter the generated key. The key stays in tab
memory, is sent only as an Authorization header, and is not persisted in cookies,
localStorage, or sessionStorage. Disconnecting or reloading clears the session and
returns to the labeled demo. Live errors never silently substitute demo data.

The dashboard's read-only Next.js proxy defaults to `http://127.0.0.1:8080`.
For another Rust API origin, create `web/.env.local`:

```dotenv
SIREN_API_URL=http://127.0.0.1:8080
```

Restart Next.js after changing the environment. Do not put API keys in
`NEXT_PUBLIC_*` variables. The proxy only forwards allowlisted read endpoints to
this fixed origin, rejects redirects, disables caching, and caps response size and
time. It does not expose mutation or response-execution endpoints.

## Views and data scope

- **Overview:** recent incident counts, an hourly Highcharts activity chart, and
  network capability probe status. Counts describe the latest **256 updated
  incidents**, not all-time activity or total chain activity. The chart buckets
  those incidents by their observation timestamps, including retracted records in
  the overall series; the priority series excludes retractions.
- **Findings:** search by type, address, incident ID, or network; filter by chain,
  severity, and status; sort by observation time; paginate and export the filtered
  snapshot to JSON. Exported demo data is explicitly marked synthetic.
- **Incident investigation:** evidence, confidence, finality, simulation status,
  current JSON, and revision history. Revisions load oldest first in pages of 20.
  Use Refresh history to see later revisions.
- **Networks:** latest capabilities, recent/stale probes, and degradation reasons.
  A reporting probe is not a claim of complete historical or trace coverage.
- **Address explorer:** per-chain watch profiles, raw funding amounts in base
  units, history completeness, and deployments.

Live incidents refresh every 15 seconds; capabilities and readiness every 30
seconds while the tab is active. Use Pause updates to pause these dashboard polls.
An open incident drawer independently polls its current revision every 15 seconds.
API rate-limit and authentication errors are surfaced. Multiple open dashboards
share the Rust key's rate limit. No account management or write actions are added.

## Build and verify

```sh
pnpm typecheck
pnpm build
pnpm start
```

Browser tests are provided but were not executed. Start the dashboard yourself,
then, if desired, install Playwright's test browser and run:

```sh
pnpm exec playwright install chromium
pnpm test
```

Tests use `http://127.0.0.1:3000`, or `DASHBOARD_TEST_URL` if set. The test configuration
never starts a server. Live-data scenarios mock the API at the browser boundary;
they do not contact providers or mutate a database. The proxy guard tests exercise
only rejected requests. Provider-backed integration remains a separate check.

Highcharts is included at the user's request and retains its own license terms;
review its licensing for your intended deployment. The repository's MIT license
does not relicense third-party dependencies.
