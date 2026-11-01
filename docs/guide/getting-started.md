# Getting started

Run all commands from the project root unless stated otherwise.

## Prerequisites

- An x86_64 Linux Docker host with Docker Compose.
- Python 3.10 or newer for the setup wizard.
- `gpg` and `gpgconf` for repository signing.
- Network access to Arch package mirrors and your configured package sources.
- An S3 hostname reachable from build containers **and** pacman clients.
- HTTPS reverse proxies for the repo and S3 APIs before using them over the internet.

The default setup uses a shared SQLite database and one Garage node. Initial image
builds can take several minutes and require space for Docker layers and packages.

## Interactive setup

```sh
git clone https://github.com/neferin12/aur-builder.git
cd aur-builder
./setup.py
```

The wizard:

1. Creates or updates `.env` with owner-only permissions and generates missing secrets.
2. Creates `config.yaml` for AUR and Git sources if the file does not already exist.
3. Asks whether to build images and start Compose services.
4. Starts Garage and RabbitMQ, provisions a private bucket, and saves S3 credentials.
5. Generates or imports a signing key and exports `repo-public-key.asc`.
6. Builds application images and optionally a local Arch build-container image.
7. Starts the server, waits for migrations, and starts worker, repo, and web services.
8. Optionally configures SMTP notifications.

::: tip The deployment question defaults to No
At `Build images and start Docker Compose services now? [y/N]:`, type **y** to
continue into Garage and GPG provisioning. Pressing Enter saves configuration and
exits before the signing prompt. Rerun the wizard to continue.
:::

Existing files, Garage access keys, and signing keys are reused. The wizard never
deletes queues or volumes. Existing RabbitMQ users are not changed by editing `.env`.
If the wizard offers to switch your database URL to Compose's SQLite path, it does
not move the contents of an existing database.

### Configuration only

```sh
./setup.py --configure-only
```

This writes environment and package configuration without running Docker or
provisioning signing keys. Run full setup later to complete provisioning.

## Manual setup

### Prepare configuration

```sh
cp .env.template .env
chmod 600 .env
mkdir -p db server_tmp
```

Set broker credentials before its first startup:

```dotenv
DATABASE_URL=sqlite:/app/db/sqlite.db?mode=rwc
RABBITMQ_DEFAULT_USER=builder
RABBITMQ_DEFAULT_PASS=REPLACE_WITH_A_PASSWORD
AMQP_ADDR=amqp://builder:URL_ENCODED_PASSWORD@rabbitmq:5672/%2f
```

Use a non-`guest` broker account. Percent-encode reserved characters in AMQP
credentials. Follow [Configuration](../reference/configuration.md) to create
`config.yaml`, then [Garage storage](./storage.md) and [GPG signing](./signing.md)
to provision storage and the signing home.

### Build and start services

```sh
docker compose build server worker web repo
docker compose up -d garage rabbitmq
docker compose up -d server
docker compose logs -f server
```

Wait for migrations to finish, then leave the log viewer with Ctrl+C and start the
remaining services:

```sh
docker compose up -d worker repo web
```

Workers also need a matching [build-container image](./development.md#build-container).
For v2 use the v2 image or a local image built by setup. SMTP notifications are
optional; leave `notifier` stopped until its environment is configured.

## After startup

| Service | Default host port | Use |
| --- | --- | --- |
| Web | `3000` | Package dashboard and logs |
| Repo | `3001` | Authenticated pacman endpoints |
| Garage | `3900` | S3 API |
| RabbitMQ | `5672` | Internal message transport |
| RabbitMQ management | `15672` | Internal administration |

Check services with `docker compose ps` and `docker compose logs server worker repo`.
Configure [Nginx](./nginx.md) and [pacman clients](./pacman.md). The repository
returns 404 for its database until the first successful publication.
