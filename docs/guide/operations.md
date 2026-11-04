# Operations

## Inspect services and builds

```sh
docker compose ps
docker compose logs --tail 100 server worker repo
```

The web dashboard lists packages and latest recorded build status. Open a package
to inspect its history, read worker output, or request a rebuild. Rebuilds queue
the latest approved release with its pinned source and settings; they do not
immediately start a container. Queue wait and build time affect when a result appears.

If a rebuild returns HTTP 409, the package has no approved release. Open **Release
reviews** and ask an administrator or a user with approval permission to approve a
candidate first. Approval queues the initial build automatically. Packages carried
over from an older installation also need approval before they can be rebuilt.

Publication has its own logs. If a worker build succeeded but the package is not
available, inspect `repo` logs and ingestion queues.

## Build container cleanup

After collecting final logs, workers remove build containers and their anonymous
volumes for both successful and failed builds. They also attempt cleanup after
startup errors or an unexpectedly closed Docker wait stream, using force removal
to stop containers that may still be running. Automatic removal on exit stays
disabled so logs can be collected first. Recorded build logs and uploaded Garage
bundles remain available after container deletion.

Cleanup failures appear in worker logs. A worker crash or an unavailable Docker
daemon can leave a container behind. Inspect leftover containers on the worker's
Docker host:

```sh
docker ps -a --filter 'name=build-'
```

After confirming that a specific container no longer belongs to an active build,
remove it and its anonymous volumes with `docker rm -f -v CONTAINER_ID`.

## Inspect queues

```sh
docker compose exec -T rabbitmq rabbitmqctl list_queues name messages consumers
```

Transient storage or signing failures retry. Invalid bundles and malformed
ingestion messages are preserved in `repo_ingest_failed`. Inspect the cause before
retrying a failed artifact. For changed package bytes under a previously published
filename, increase `pkgrel` or `pkgver` and rebuild.

## Backups

Keep these together:

- `./db/`: package metadata, build logs, attempts, migrations, and repository catalog.
- Garage metadata and data volumes: private build and published objects.
- `./repo-gnupg/`: repository signing secret key and public key material.
- `.env`, `config.yaml`, and `garage.toml`: deployment configuration and credentials.

For a consistent offline backup, let builds finish and stop writers before copying
state. Protect backups containing credentials or signing keys. Restore the SQL
catalog and Garage data as a matched snapshot before restarting writers.

## Reset the Compose SQLite database

::: danger This removes recorded history and publication state
A database reset removes package metadata, logs, task attempts, and the repository
catalog. Garage objects and GPG keys remain, but old packages are no longer listed
by the new catalog. Rebuild configured packages to repopulate the repository.

The commands below assume `DATABASE_URL=sqlite:/app/db/sqlite.db?mode=rwc` and the
standard `./db:/app/db` mount. For a different database, use its own backup/reset
procedure instead.
:::

Let builds finish first, then stop database users and message producers:

```sh
docker compose stop server worker web repo notifier
```

Keep RabbitMQ running and purge messages referring to the old database:

```sh
for queue in pkg_build build_results expired_tasks repo_ingest repo_ingest_failed notifications; do
    docker compose exec -T rabbitmq rabbitmqctl purge_queue "$queue"
done
```

A queue that does not exist can be skipped. Move the database directory into a
backup, preserving SQLite sidecar files, then create a fresh directory:

```sh
sudo mv db "db.backup-$(date +%Y%m%d-%H%M%S)"
mkdir db

docker compose up -d server
docker compose logs -f server
```

After migrations finish, press Ctrl+C and start the remaining services:

```sh
docker compose up -d worker repo web
# If email notifications are configured:
docker compose up -d notifier
```

The server rediscovers packages from `config.yaml`. The repository is empty until
publication succeeds again.

::: warning Keep Garage volumes
Do not use `docker compose down -v` as a database-reset shortcut. It also deletes
the named Garage and RabbitMQ volumes.
:::
