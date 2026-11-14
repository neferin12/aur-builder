# Development & CI

## Devcontainer

For a preconfigured Node/Rust workspace with RabbitMQ, Keycloak test users,
Mailpit, Garage, and PostgreSQL, see the [devcontainer setup](https://github.com/neferin12/aur-builder/tree/main/.devcontainer#readme).
It uses isolated development databases and retains HTTPS/OIDC checks.

## Rust workspace

Use a Rust toolchain supporting edition 2024, a C compiler, `pkg-config`, and the
OpenSSL/libgit2 development dependencies required by native builds. Run commands
from the project root so asset and template paths resolve:

```sh
cargo check --workspace
cargo test --workspace
```

The full test command includes database doctests using in-memory SQLite. External
service integration tests are ignored unless explicitly enabled.

## Nuxt web application

Use Node.js 24 and npm. The UI uses PrimeVue and oRPC; Nitro implements all web
server behavior in TypeScript. The scheduler and other services remain Rust.

Apply the Rust-owned migrations before starting Nuxt. For a database under the
repository root:

```sh
DATABASE_URL='sqlite:./db/sqlite.db?mode=rwc' cargo run -p database --example migrate
cd web
npm ci
# Load the existing OIDC/storage settings from ../.env. SQLite paths are resolved
# relative to the process working directory, so point back to the root's db/.
DATABASE_URL='sqlite:../db/sqlite.db?mode=rwc' npm run dev -- --dotenv ../.env
```

OIDC still requires the configured HTTPS origin. Put the development server behind
an HTTPS reverse proxy and configure `AB_WEB_URL` accordingly. Production runs
`npm run build` followed by `npm start`; use the existing Docker `web` target for
container deployments. Compose runs the one-shot `migrate` service before web.
Nuxt fails startup if the compatibility migration has not been applied.

```sh
cd web
npm run typecheck
npm test
npm run build
npx playwright install chromium
npm run test:e2e
```

Vitest creates temporary SQLite fixtures using the Rust migrations. Set
`TEST_POSTGRESQL_URL` to a **disposable database** to run the same integration
suite against PostgreSQL; its fixture tables are cleared. Browser tests start an
isolated HTTPS Nuxt deployment and a signed local OIDC provider. The committed
localhost key/certificate are public test fixtures only.

### Schema-derived database client

Prisma 6 generates separate SQLite and PostgreSQL clients with `engineType = "client"`.
JavaScript database adapters perform queries; Prisma’s query compiler runs as
WebAssembly. The web service contains no native Rust query engine. Rust migrations are the
schema authority: never run `prisma migrate` or `prisma db push` against the shared
database. To refresh the committed Prisma schemas, first migrate disposable databases,
then run:

```sh
cd web
PRISMA_SQLITE_URL='file:/absolute/path/to/disposable.db' \
PRISMA_POSTGRESQL_URL='postgresql://user:password@localhost/disposable' \
  npm run db:introspect
npm run db:generate
```

`npm run db:check` uses the same variables and fails on schema drift. CI runs it
against fresh Rust-migrated databases. The introspection script makes existing
SQLite text primary keys required for Prisma (application writers always supply
UUIDs), and omits a relation whose legacy foreign-key scalar width differs from
its target. Scalar column types remain schema-derived. The typed database adapter
normalizes provider delegate interfaces without handwritten row types. The
SQLite compatibility migration changes timestamp column declarations to TEXT
without changing stored timestamps, so both Prisma and SeaORM can read them.

Application requests use the inferred oRPC client. Authentication redirects are
ordinary Nitro routes. Mutations require POST, the configured origin, a current
session, the required permission, and the session's CSRF token. Upload grants,
environment secrets, and full task payloads never enter frontend responses.

Native repo processes need `gpg`, `bsdtar`, and `repo-add`; the repo Docker image
includes these tools.

## Build container

```sh
docker build -t your-registry/aur-builder-build-container:your-tag build-container
```

Publish the image to a registry reachable by worker Docker hosts, or build it on
the same host used by workers. Configure `AB_BUILDER` and `AB_BUILDER_TAG` to match.

| Container variable | Source |
| --- | --- |
| `AB_SOURCE` | Task Git URL |
| `AB_SUBFOLDER` | Optional package subdirectory |
| `AB_OPTIONS` | Optional `makepkg` arguments |
| `AB_REVISION` | Exact approved commit |

The container also receives allowed `AB_PKG_` per-package environment entries. It does not receive
S3 credentials, upload grants, receipt keys, or signing keys. It writes a bounded
bundle and completion record; the worker exports the bytes, freezes the sandbox,
and uploads an authenticated result. Deploy updated builder images alongside matching
workers.

The builder is a standalone Rust crate under `build-container`, excluded from the
root workspace because it requires Arch Linux's libalpm. It uses Paru's underlying
`aur-depends`, `aur-fetch`, `raur`, and `srcinfo` libraries, with pacman for package
installation and makepkg for builds. The Docker build runs its tests and compiles
the binary in an Arch stage; the runtime image contains neither yay nor Rust.

On an Arch host with Rust and base-devel installed:

```sh
cargo test --manifest-path build-container/Cargo.toml --locked
cargo clippy --manifest-path build-container/Cargo.toml --locked -- -D warnings
```

Dependencies are resolved recursively with version constraints, split packages,
providers, build dependencies, and native architecture metadata. AUR dependency
builds always skip checks and check-only dependencies; target checks follow
makepkg defaults and `AB_OPTIONS`. Ambiguous AUR providers, missing dependencies,
and blocked dependency cycles fail without prompting.

`AB_OPTIONS` is split on ASCII whitespace into literal arguments; quotes and
wildcards receive no shell interpretation. Both build output streams are captured
in the build log. Existing package archives from `makepkg --packagelist` are bundled;
signatures and absent optional debug archives are excluded. A missing upload URL
fails before cloning sources with exit code 107.

The worker collects final logs before removing the container and its anonymous
volumes. See [container cleanup](./operations.md#build-container-cleanup) for failure
handling and operational limits.

| Exit code | Meaning |
| --- | --- |
| 100 | Cannot change directory |
| 101 | Source environment variable missing |
| 102 | Git clone failed |
| 103 | Package update failed |
| 104 | Metadata, dependency resolution, or installation failed |
| 105 | Build failed |
| 106 | Cannot collect outputs or create bundle |
| 107 | Bundle upload failed |


## Setup script tests

```sh
python3 -m unittest discover -s scripts -p test_setup.py
```

The opt-in test starts and cleans up a disposable Garage Compose project, creates a
temporary GPG key, checks reruns, and verifies dotenv handling:

```sh
AB_SETUP_INTEGRATION=1 python3 -m unittest discover -s scripts -p test_setup_integration.py
```

It requires Docker, the Garage image, GPG, and Compose.

## Storage and broker integration tests

Use a **disposable** Garage bucket and signing home with the normal `AB_S3_*`,
`GNUPGHOME`, and `AB_REPO_GPG_KEY` variables configured:

```sh
cargo test -p repo private_signed_publication -- --ignored --nocapture
```

The test writes artifacts. It requires `gpg`, `bsdtar`, `repo-add`, `pacman`, and
`fakeroot`, and checks private access, signed requests, publication, split packages,
retained downloads, and pacman installation into a temporary root.

With a disposable RabbitMQ instance selected by `AMQP_ADDR`:

```sh
cargo test -p common routed_confirmations -- --ignored
```

This tests routed publisher confirmations and dead-letter behavior.

## Container release CI

The application image workflow runs on `v*.*.*` tags. It includes `repo` and
publishes images to GHCR and Harbor, with semver tags and the existing image
signing, SBOM, and provenance steps. Harbor uses `HARBOR_USER` / `HARBOR_PASS`
repository secrets; GHCR uses `GITHUB_TOKEN`.

The separate builder-container workflow publishes the Arch image on release tags
and also has its own scheduled/manual triggers. Use matching v2 application and
builder images when deploying the new task protocol.

## Documentation

The documentation uses `vitepress@next` in an independent project under `docs/`.
The committed lockfile pins the resolved prerelease for reproducible builds. Use Node.js
22.12 or newer and npm:

```sh
npm --prefix docs ci
npm --prefix docs run dev
```

Build and preview the static site:

```sh
npm --prefix docs run build
npm --prefix docs run preview
```

The output directory is `docs/.vitepress/dist`. Broken internal page links fail
the build. Local search is generated at build time and requires no external service.
For a subpath deployment, build with a leading/trailing-slash base:

```sh
DOCS_BASE=/aur-builder/ npm --prefix docs run build
```

Serve the output directory with a static web server, or use the GitHub Pages
workflow included in this repository.

### GitHub Pages deployment

In repository **Settings → Pages**, set **Build and deployment → Source** to
**GitHub Actions**. Commit the documentation and workflow to the default branch.

The **Documentation** workflow tests and builds the site on documentation changes.
Successful pushes to the repository's default branch deploy it to GitHub Pages.
You can also select **Run workflow** on the default branch to deploy manually.
Pull requests and other branches only run build checks; they cannot publish.

The workflow obtains the base path from `actions/configure-pages`, so project sites
such as `https://neferin12.github.io/aur-builder/` use `/aur-builder/` automatically.
A custom domain configured in Pages uses the corresponding root path. Local builds
continue to default to `/` unless `DOCS_BASE` is set explicitly.

Only the deploy job receives `pages: write` and `id-token: write` permissions. Its
`github-pages` environment records the published URL and applies any configured
environment protection rules. Existing deployments are not cancelled by newer
pushes; pull-request build checks can be cancelled when superseded.

### Mermaid diagrams

Use a fenced `mermaid` block in any documentation page. A local Markdown extension
renders it through the Mermaid Vue component, with automatic light/dark theme
switching. Mermaid is loaded only in the browser; source text remains available
while loading or if rendering fails. Add `accTitle` and `accDescr` to describe each
diagram for assistive technology.
