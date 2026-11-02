# Configuration

Use `.env` for service credentials and `config.yaml` for package sources. The
[setup wizard](../guide/getting-started.md#interactive-setup) can generate both.
Do not commit `.env` or `repo-gnupg/`.

## Package sources

```yaml
sleepduration: 300
keep_versions: 2

aur_packages:
  - name: example-aur-package
    options: "--nocheck"
    env:
      - name: MAKEFLAGS
        value: "-j2"

git_packages:
  - source: https://example.com/packaging.git
    subfolder: arch/example
    options: "--nocheck"
```

Replace example sources with real packages. Both source lists must be present;
use `[]` for an unused list. Restart the server after changing this file.

| Field | Meaning |
| --- | --- |
| `keep_versions` | Published releases to retain per package base; defaults to `2`, minimum `1`. Cleanup runs each server loop; expired packages and signatures are deleted. Build bundles and history are retained. |
| `sleepduration` | Source-check interval in seconds; defaults to `300` |
| `aur_packages[].name` | AUR package to track |
| `git_packages[].source` | Git clone URL |
| `git_packages[].subfolder` | Optional package directory within the checkout |
| `options` | Optional arguments passed to `makepkg` |
| `env` | Optional list of `{ name, value }` environment entries passed to the build |

Git packages need `.SRCINFO`, `PKGBUILD`, and supporting files in the selected
directory. Build options use shell word splitting; do not use them to supply secrets.

## General services

| Variable | Purpose / default |
| --- | --- |
| `RUST_LOG` | Logging level; template uses `info` |
| `DATABASE_URL` | Compose: `sqlite:/app/db/sqlite.db?mode=rwc` |
| `AMQP_ADDR` | Broker URL, e.g. `amqp://builder:ENCODED_PASSWORD@rabbitmq:5672/%2f` |
| `RABBITMQ_DEFAULT_USER` | Initial broker user; use a non-`guest` account |
| `RABBITMQ_DEFAULT_PASS` | Initial broker password |
| `AB_CONFIG_PATH` | Server package-config path; Compose overrides this to `/config.yaml` |

Compose mounts `./db` at `/app/db` for server, repo, and web. A native process can
use `sqlite:./db/sqlite.db?mode=rwc`. The server applies migrations at startup.
Changing RabbitMQ's initialization variables does not change existing broker users.

Services load `.env`; existing process environment variables take precedence over
values loaded from that file. In typed configuration, the optional configuration
file is loaded after environment settings and can override them. Worker settings
can also be supplied in a worker configuration file selected with `AB_CONFIG_PATH`.

## Garage and presigned URLs

| Variable | Purpose / default |
| --- | --- |
| `GARAGE_RPC_SECRET` | 64-character hex cluster secret; setup generates it |
| `AB_S3_ENDPOINT` | Internal S3 origin: `http://garage:3900` |
| `AB_S3_PUBLIC_ENDPOINT` | HTTP(S) origin reachable by workers and clients; use HTTPS publicly |
| `AB_S3_BUCKET` | Private bucket; setup default `aur-builder` |
| `AB_S3_REGION` | Garage region: `garage` |
| `AB_S3_ACCESS_KEY` | Garage-issued S3 access key |
| `AB_S3_SECRET_KEY` | Matching Garage-issued secret |
| `AB_UPLOAD_TTL` | Upload URL lifetime: `86400` seconds (24 hours) |
| `AB_DOWNLOAD_TTL` | Download URL lifetime: `900` seconds (15 minutes) |
| `AB_REPO_MAX_BYTES` | Bundle download/extracted package limit: `4294967296` bytes (4 GiB) |

URL lifetimes must be between 1 and 604800 seconds. Endpoint URLs must be origins,
without credentials, query strings, fragments, or path prefixes. Internal and
external origins may differ. If no public origin is supplied, signing falls back
to the internal origin, which may not be reachable by clients.

## Repository and signing

| Variable | Purpose / default |
| --- | --- |
| `AB_REPO_NAME` | Repository name and URL segment; `aur-builder` |
| `AB_REPO_USER` | Required, nonempty Basic-auth username |
| `AB_REPO_PASSWORD` | Required, nonempty Basic-auth password |
| `AB_REPO_GPG_KEY` | Full fingerprint of the signing secret key |
| `GNUPGHOME` | Repo's GPG home; Compose uses `/run/repo-gnupg` |

Compose mounts `./repo-gnupg` read-only and copies it to a temporary writable home
when the repo container starts. Recreate the repo container after changing key
material. Never give the signing key to workers.

## Worker image

| Variable | Purpose / default |
| --- | --- |
| `AB_BUILDER` | Build-container image; `ghcr.io/neferin12/aur-builder-build-container` |
| `AB_BUILDER_TAG` | Compose defaults to `2`; native worker defaults to its package version |

Setup can build `aur-builder-build-container:setup-local` and save those overrides.
The build image must exist on the worker's Docker host or be pullable from a
registry. Workers attempt a pull before each build and can use an existing local
image if pulling fails. The worker needs the Docker socket but no storage credentials.

## Optional SMTP notifications

```dotenv
AB_SMTP_HOST=smtp.example.com
AB_SMTP_USER=builder@example.com
AB_SMTP_PASS=REPLACE_WITH_A_PASSWORD
AB_SMTP_FROM=builder@example.com
AB_SMTP_TO=you@example.com
AB_MAILLOGO=https://example.com/logo.png
```

Configure all fields, then run `docker compose up -d --build notifier`.
Leave the notifier stopped if you do not use email notifications.

## Release reviews, authentication, and AI

The web UI requires `AB_WEB_URL`, `AB_OIDC_ISSUER`, `AB_OIDC_CLIENT_ID`,
`AB_OIDC_CLIENT_SECRET`, and `AB_OIDC_ADMIN_GROUP`. `AB_OIDC_GROUPS_CLAIM` defaults
to `groups`; `AB_OIDC_SCOPES` defaults to `profile` (space-separated, with `openid`
always included); `AB_SESSION_SECONDS` defaults to `3600` (range 60–86400). Both web URL
and issuer require HTTPS. OIDC discovery, authorization, token and signing-key
endpoints must use HTTPS without URL credentials. The callback is `/auth/callback` on the configured web origin.

`AB_AI_MODE` accepts `disabled` (default), `feedback`, or `auto_approve`.
Enabled modes require `AB_AI_MODEL` and `OPENAI_API_KEY`. `AB_AI_MAX_BYTES` defaults
to `200000`; `AB_AI_TIMEOUT_SECONDS` defaults to `90`. Settings are environment
variables, loaded at service startup. Workers now require `DATABASE_URL` and access
to the same database as server, web, and repo.

See [Release reviews and SSO](../guide/reviews) for approval policy and setup.

Changing the OIDC identity settings, client credentials, web origin, scopes, group
mapping or session lifetime invalidates prior sessions and pending login flows.
Set identical values across web replicas. Signing-key refresh is automatic with a
five-minute cache and a one-minute minimum interval between refresh attempts.
