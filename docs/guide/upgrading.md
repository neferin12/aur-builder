# Upgrade to v2

V2 replaces direct Gitea uploads with private Garage storage, task-specific upload
URLs, and a dedicated signed pacman repository. Treat server, worker, and
build-container deployment as a coordinated upgrade.

## Before deployment

Back up the database, Garage state if already configured, configuration, and GPG
keys. Finish running builds and stop server and workers before changing task
formats or queue definitions.

Provision [Garage](./storage.md), [signing](./signing.md), and [HTTPS endpoints](./nginx.md).
Existing Gitea package files are not imported automatically.

## Images

The supplied Compose file uses major-version `:2` application images, including
`ghcr.io/neferin12/aur-builder-repo:2`. Its default build-container tag is also `2`.
If your environment overrides `AB_BUILDER_TAG`, ensure that image supports the v2
presigned bundle-upload protocol.

To use published images:

```sh
docker compose pull server worker web repo
# If enabled:
docker compose pull notifier
```

Alternatively, build matching application images from this checkout with
`docker compose build server worker web repo`. Build or pull the matching Arch
build-container image on each worker's Docker host.

## Queue compatibility

The task wire format changed and queues are now durable. `repo_ingest` also uses a
dead-letter routing policy. RabbitMQ cannot change durability or queue arguments
in place.

Before starting the new producers, drain the old queues. Inspect message and
consumer counts in RabbitMQ's management interface or with:

```sh
docker compose exec -T rabbitmq rabbitmqctl list_queues name messages consumers
```

Delete only the old, empty, unused queue definitions that need replacement:
`pkg_build`, `build_results`, `notifications`, and, when upgrading an earlier v2
setup, `repo_ingest`. Starting the new services recreates them with their expected
properties. Do not delete queued work unless you deliberately intend to discard it.

The setup wizard preserves queues and volumes; it does not perform this migration.

## Start and verify

Start the server first and wait for its database migrations:

```sh
docker compose up -d garage rabbitmq server
docker compose logs -f server
```

Then start worker, repo, and web services, plus notifier if configured. Request
rebuilds through the web UI to repopulate the repository from packages previously
uploaded to Gitea.

Verify a successful publication, import the public key on clients, and use the
[new pacman repository entry](./pacman.md). A database request returning 404 before
the first publication is expected.

## Upgrade to release approvals and OIDC

1. Stop detection and drain existing build/result/ingestion queues with the old
   services. Back up the shared database. Stop the remaining services.
2. Configure the mandatory OIDC settings and HTTPS web origin described in
   [Release reviews and SSO](./reviews). Register the callback with your provider.
3. Rebuild all application images **and the build-container image** together.
   Workers now need the shared database; update Compose mounts and environment.
4. Start the server to apply additive migrations, then start the other services.
5. Sign in using the administrator group and approve the first captured packaging
   snapshot for every existing package. Existing published packages remain available.

Legacy unpinned tasks and results are rejected after upgrading. Do not mix old and
new service versions or expect queued old attempts to publish. Pending approvals,
users, and sessions live in SQL; protect database backups because candidate build
payloads contain configured environment settings. Rollback requires a coordinated
service rollback and restoration of the database backup after stopping all writers.

### Authentication hardening migration

The additive authentication migration clears existing web sessions and in-progress
OIDC logins once. Users must sign in again. It also backfills the last observed source
fingerprint so that source reverts supersede obsolete pending reviews correctly.
Published artifacts, permissions, release candidates, decisions and notification
records are retained. Back up the database and update server and web together; do
not mix old authentication binaries with the new session storage format.
