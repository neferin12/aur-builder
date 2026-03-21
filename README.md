# AUR Builder

A Rust-based microservices platform for automatically building and publishing [Arch User Repository (AUR)](https://aur.archlinux.org/) packages. Services communicate via RabbitMQ and store state in a SQLite or PostgreSQL database.

## Architecture

```
Server (monitors AUR/git)
    │  publishes BuildTask
    ▼
RabbitMQ (pkg_build queue)
    │  consumed by
    ▼
Worker (builds in Docker container)
    │  publishes BuildResult
    ▼
RabbitMQ (build_results queue)
    ├──► Server (saves to DB)
    └──► Notifier (sends email)
              │
              ▼
         SMTP (email)

Database (SQLite / PostgreSQL)
    └──► Web UI (port 3000)
```

### Microservices

| Service | Description |
|---------|-------------|
| **server** | Periodically polls AUR and git repositories for package updates; dispatches build tasks |
| **worker** | Pulls build tasks and executes them inside isolated Docker containers (runs with 2+ replicas) |
| **web** | HTTP UI on port 3000 — lists packages, shows build history, allows force-rebuilds |
| **notifier** | Sends HTML email notifications on build success or failure |
| **database** | Shared library — Sea-ORM entities, migrations, and database access layer |
| **common** | Shared library — types, config, error codes, RabbitMQ helpers |

## Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)
- Docker & Docker Compose (for running locally)
- A [Gitea](https://gitea.io/) instance to receive published packages
- An SMTP server for email notifications (optional)

## Local development

```bash
# 1. Copy and edit environment variables
cp .env.template .env
# Fill in COMPOSE_RABBIT_MQ_PASSWORD, AB_GITEA_*, DATABASE_URL, etc.

# 2. Create a server config file (see Configuration below)
cp config.yaml.example config.yaml   # edit as needed

# 3. Start all services
docker compose up
```

The web UI is available at <http://localhost:3000>.  
The RabbitMQ management UI is available at <http://localhost:15672>.

## Configuration

The **server** service is configured via a YAML file (path set with `AB_CONFIG_PATH`):

```yaml
aur_packages:
  - name: firefox-bin
  - name: chromium
    options: "--sign"
    env:
      - name: SOME_VAR
        value: some_value

git_packages:
  - source: https://github.com/example/my-aur-package
    subfolder: my-pkg   # optional – if .SRCINFO is in a subdirectory

sleepduration: 300  # seconds between update checks (default: 300)
```

### Environment variables

| Variable | Description |
|----------|-------------|
| `DATABASE_URL` | Database connection string, e.g. `sqlite:./sqlite.db?mode=rwc` or `postgres://…` |
| `AMQP_ADDR` | RabbitMQ connection string (default: `amqp://127.0.0.1:5672/%2f`) |
| `AB_CONFIG_PATH` | Path to the server YAML config file |
| `AB_GITEA_REPO` | Gitea repository URL to publish packages to |
| `AB_GITEA_USER` | Gitea username |
| `AB_GITEA_TOKEN` | Gitea access token |
| `RUST_LOG` | Log level (e.g. `info`, `debug`) |

## Building

```bash
# Check all packages compile
cargo check

# Build all release binaries
cargo build --release
```

## Running the tests

The test suite uses in-memory SQLite databases and HTTP mocks — no external services are required.

```bash
# Run all tests across the entire workspace
cargo test --workspace

# Run tests for a specific package
cargo test --package common
cargo test --package database
cargo test --package server
cargo test --package notifier
cargo test --package worker
cargo test --package web

# Run a single test by name
cargo test --workspace -- test_get_error_descriptions_success
```

### Test coverage

| Package | Tests | What is covered |
|---------|------:|-----------------|
| `common` | 47 | Error types, error codes, env vars, config parsing, type serialization |
| `database` | 19 | CRUD, metadata update logic, build result storage, timestamp ordering |
| `server` | 14 | AUR HTTP responses (mocked), git repo parsing |
| `notifier` | 7 | Template rendering, CSS inlining, email subject generation |
| `worker` | 9 | Log formatting, source URL construction, Docker image name config |
| `web` | 10 | Tera filter, HTTP route handlers, 404 handling, force-rebuild side effect |

## Build container exit codes

| Code | Description |
|------|-------------|
| 0 | Success |
| 100 | Unable to change dir |
| 101 | Environment variable missing |
| 102 | Git clone failed |
| 103 | Failed to run `yay -Syu` |
| 104 | Failed to install dependency |
| 105 | Failed to build package |
| 106 | Failed to copy result files |
| 107 | Failed to upload pkg file |

## License

See [LICENSE](LICENSE) for details.
