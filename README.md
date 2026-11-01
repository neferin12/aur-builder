# AUR Builder

A self-hosted build system for Arch Linux packages. Track packages from the AUR or
Git repositories, build them on Docker workers, and publish them to a private,
GPG-signed pacman repository.

The web dashboard shows package versions and latest build results, provides
searchable build logs, and lets you request rebuilds. Email notifications are
available through the optional notifier service.

## Documentation

The VitePress documentation covers setup, configuration, Garage, GPG signing,
pacman clients, Nginx, database reset, troubleshooting, and v2 upgrades.
Start with the [getting-started guide](docs/guide/getting-started.md).

To browse locally with Node.js 22.12+:

```sh
npm --prefix docs ci
npm --prefix docs run dev
```

Build a static site with `npm --prefix docs run build`. See
[documentation development](docs/guide/development.md#documentation) for preview,
subpath hosting, and CI details. The Documentation workflow deploys successful
default-branch builds to GitHub Pages; first set **Settings → Pages → Source** to
**GitHub Actions**.

## How it works

1. The server checks configured sources for changes and records build tasks in the
   database before publishing them to RabbitMQ.
2. A worker starts an Arch Linux build container. Each task includes a presigned
   URL for uploading its package bundle to Garage; workers need no S3 credentials.
3. The server records the result and forwards successful builds for publication.
4. The repo service validates the bundle, signs packages and repository databases,
   uploads them to private storage, and switches the published catalog.
5. Pacman clients authenticate with the repo service and download artifacts through
   short-lived presigned URLs.

A successful build and successful repository publication are separate steps.
Publication failures leave the previous repository available.

| Component | Purpose |
| --- | --- |
| `server` | Source checks, task scheduling, upload grants, and result processing |
| `worker` | Runs build containers and reports results |
| `build-container` | Fetches sources, installs dependencies, runs `makepkg`, and uploads bundles |
| `repo` | Package validation, GPG signing, repository publication, and authenticated downloads |
| `web` | Package dashboard, build history, logs, and rebuild requests |
| `notifier` | Optional SMTP build notifications |
| `database` | Shared database access and migrations |
| `common` | Configuration, message formats, messaging, and storage signing |

RabbitMQ carries tasks and results. Garage stores private artifacts. SQL stores
package metadata, build logs, task attempts, and the published repository catalog.

## Getting started with Docker Compose

You need Docker with Compose, access to an x86_64 Docker host for workers, and a GPG
signing key for repository publication. The supplied deployment uses a single
Garage node and a shared SQLite database.

### Interactive setup

With Python 3.10+, Docker Compose, `gpg`, and `gpgconf` installed, run:

```sh
./setup.py
```

The wizard configures `.env` with owner-only permissions, optionally creates
`config.yaml` for AUR and Git sources, generates missing passwords, initializes a
private Garage bucket, and generates or imports an unattended GPG signing key.
It exports `repo-public-key.asc`, builds application images and optionally the
Arch build-container image, then starts the server before its dependent services.
SMTP setup is optional. Generated signing keys expire after one year; renew or
rotate them before expiry. Allow time for the first image builds.

Existing configuration, Garage keys and signing keys are reused. The wizard never
deletes queues or volumes. Changing RabbitMQ passwords in `.env` does not update
users on an existing broker; use the existing credentials or update that broker
manually. When switching an existing database URL to Compose's SQLite path, the
wizard asks explicitly; it does not migrate data.

To prepare configuration without starting Docker or generating signing keys:

```sh
./setup.py --configure-only
```

The S3 origin you enter must resolve from workers and clients. Configure your HTTPS
reverse proxy and client key trust separately. After an interrupted run, rerun the
wizard to resume. For upgrades, follow the queue migration steps in
[STORAGE.md](STORAGE.md#rollout-and-recovery) before starting services.

### Manual setup

Follow the steps below if you prefer to configure services individually.

#### 1. Configure services

```sh
cp .env.template .env
mkdir -p db server_tmp
```

Edit `.env`. In particular, set:

```dotenv
DATABASE_URL=sqlite:/app/db/sqlite.db?mode=rwc
RABBITMQ_DEFAULT_USER=builder
RABBITMQ_DEFAULT_PASS=replace-with-a-password
AMQP_ADDR=amqp://builder:replace-with-a-password@rabbitmq:5672/%2f
```

Percent-encode reserved characters in AMQP credentials. Set the RabbitMQ default
credentials before its first startup; they do not change an existing broker user.
Use `RABBITMQ_DEFAULT_USER` and `RABBITMQ_DEFAULT_PASS` for broker initialization.

Set the storage and repository variables from the template, including a Garage RPC
secret (`openssl rand -hex 32`), S3 credentials, repository credentials, and the
signing key fingerprint. The public S3 endpoint must be reachable from both build
containers and pacman clients.

#### 2. Choose packages

Create `config.yaml` in the project root:

```yaml
# Source check interval in seconds; default: 300.
sleepduration: 300

aur_packages:
  - name: example-aur-package
    options: "--nocheck"
    env:
      - name: MAKEFLAGS
        value: "-j2"

git_packages:
  - source: https://example.com/your-package.git
    subfolder: packaging/arch
    options: "--nocheck"
```

Replace the example sources with your packages. Use `aur_packages: []` or
`git_packages: []` for an unused source type. `options` and `env` are optional;
`subfolder` is optional for Git sources. Git sources must include `.SRCINFO` in the
selected directory, alongside the `PKGBUILD` and its supporting files.

Compose mounts this file into the server and sets `AB_CONFIG_PATH=/config.yaml`.
For a native server process, set `AB_CONFIG_PATH` yourself.

#### 3. Provision storage and signing

Follow [Storage and repository operations](STORAGE.md#initial-setup) to initialize
Garage, create its private bucket and access key, and provision `repo-gnupg/` with
your signing key. Configure HTTPS endpoints for the repository and public S3 API.

Garage objects remain private. Repository authentication grants a temporary
signed download URL; possession of that URL permits access until it expires.
The repository currently serves `x86_64` and `any` packages through its x86_64 path.
Run one repo service instance.

#### 4. Build and start

```sh
docker compose build server worker web repo
docker compose up -d garage rabbitmq
docker compose up -d server
```

Wait for server startup and migrations to complete:

```sh
docker compose logs -f server
```

Then start the remaining core services:

```sh
docker compose up -d worker repo web
```

Workers use a separate build-container image. When changing its Rust builder, rebuild
and deploy that image along with the worker; see [Build container](build-container/README.md).

| Endpoint | Default port |
| --- | --- |
| Web dashboard | `3000` |
| Authenticated pacman repository | `3001` |
| Garage S3 API | `3900` |
| RabbitMQ AMQP | `5672` |
| RabbitMQ management UI | `15672` |

The dashboard and rebuild endpoint have no built-in authentication. Restrict them
to your network or put authentication in front of them. Expose the repo and S3
endpoints through HTTPS; keep broker and Garage administration access internal.

## Installing packages

Distribute the repository's public GPG key through a trusted channel, then import
and trust it on each client:

```sh
sudo pacman-key --add public-key.asc
sudo pacman-key --lsign-key FULL_FINGERPRINT
```

Add a repository entry to `/etc/pacman.conf`:

```ini
[aur-builder]
SigLevel = Required DatabaseRequired
Server = https://USER:PASSWORD@repo.example.com/aur-builder/os/$arch
```

Percent-encode reserved characters in the credentials and restrict access to that
configuration. Run `sudo pacman -Syu` to refresh and install updates. The repository
becomes available after its first successful publication. See [Pacman clients](STORAGE.md#pacman-clients)
for more details.

## Email notifications

To enable the notifier, add these variables to `.env`:

```dotenv
AB_SMTP_HOST=smtp.example.com
AB_SMTP_USER=builder@example.com
AB_SMTP_PASS=replace-with-a-password
AB_SMTP_FROM=builder@example.com
AB_SMTP_TO=you@example.com
AB_MAILLOGO=https://example.com/logo.png
```

Then run `docker compose up -d --build notifier`. Leave the notifier stopped if you
do not need email notifications.

## Development

Use a Rust toolchain with edition 2024 support. Native builds require a C compiler,
`pkg-config`, and the development libraries used by OpenSSL and libgit2. Runtime
services also need their configured database, broker, and storage connections.

Run commands from the workspace root so template and asset paths resolve:

```sh
cargo check --workspace
cargo test --workspace
```

For a native web process connected to an already initialized database:

```sh
DATABASE_URL='sqlite:./db/sqlite.db?mode=rwc' cargo run -p web
```

The server applies migrations at startup; initialize the database before starting
the web UI. Native repo processes additionally need `gpg`, `bsdtar`, and `repo-add`.
The repo Docker image includes those tools. The services load `.env` from the
working directory; existing process environment variables take precedence over
`.env` values.

The setup script has isolated configuration and deployment-order tests:

```sh
python3 -m unittest discover -s scripts -p test_setup.py
```

Its opt-in integration test starts and cleans up a disposable Garage Compose
project, creates a temporary signing key, verifies resumable provisioning, and
checks special-character dotenv values in a container environment:

```sh
AB_SETUP_INTEGRATION=1 python3 -m unittest discover -s scripts -p test_setup_integration.py
```

External-service integration tests are ignored by default. Setup and commands for
Garage, GPG, pacman, and RabbitMQ tests are in [STORAGE.md](STORAGE.md#integration-test).
Use disposable test services for these tests.

## Operations and upgrades

- Request a rebuild from a package's build-history page. The server schedules it
  during its next source check; results appear when the worker finishes.
- Check `docker compose logs server worker repo` for scheduling, build, or
  publication failures. Invalid artifacts enter `repo_ingest_failed` for inspection.
- Published package filenames are immutable. Increase `pkgrel` or `pkgver` when a
  rebuild produces different bytes under an existing filename.
- Back up the SQL database, Garage data and metadata, and the signing key together.
  Old package objects are retained; automatic garbage collection is not implemented.
- When upgrading from direct Gitea uploads, coordinate server, worker, and builder
  deployment and recreate drained queues as described in
  [Rollout and recovery](STORAGE.md#rollout-and-recovery). Existing Gitea packages
  are not imported automatically.

Detailed URL lifetimes, size limits, queue recovery, signing, and storage procedures
are documented in [STORAGE.md](STORAGE.md).
