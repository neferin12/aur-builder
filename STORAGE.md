# Storage and repository operations

For the browsable VitePress guides, see [Garage storage](docs/guide/storage.md),
[GPG signing](docs/guide/signing.md), and [operations](docs/guide/operations.md).

Workers now upload one tar bundle using the PUT capability included in each task.
They need RabbitMQ access and connectivity to `AB_S3_PUBLIC_ENDPOINT`, but no S3
credentials. The server and repo service need S3 credentials. Package downloads
require Basic authentication at repo and receive a 15 minute signed redirect.
Anyone holding that redirect can use it until expiry. Do not log query strings on
the S3 reverse proxy, and use HTTPS for both externally reachable endpoints.

## Initial setup

For guided provisioning, run `./setup.py` from the project root. It configures
Compose and `.env`, provisions Garage and signing, and starts services in order.
Use `./setup.py --configure-only` to prepare configuration without deployment.
The manual equivalent follows.

1. Copy `.env.template` to `.env`, configure AMQP and the existing server settings,
   and set `DATABASE_URL=sqlite:/app/db/sqlite.db?mode=rwc` for Compose services.
   Generate `GARAGE_RPC_SECRET` with `openssl rand -hex 32`.
2. Start `docker compose up -d garage rabbitmq`. Inspect the Garage node:
   `docker compose exec garage /garage status`. Assign its node ID:
   `docker compose exec garage /garage layout assign -z local -c 10G NODE_ID`,
   then `docker compose exec garage /garage layout apply --version 1`.
3. Create a private bucket and key:
   `docker compose exec garage /garage bucket create aur-builder`,
   `docker compose exec garage /garage key create aur-builder`, then
   `docker compose exec garage /garage bucket allow --read --write --key aur-builder aur-builder`.
   Put the printed access key and secret in `AB_S3_ACCESS_KEY` and
   `AB_S3_SECRET_KEY`. Do not enable Garage website hosting or public access.
   The Compose deployment is a single-node development baseline; back up Garage
   metadata and data together before changing its layout.
4. Set `AB_S3_PUBLIC_ENDPOINT` to an HTTPS origin reachable from build containers
   and pacman clients. Forward it to Garage port 3900, preserving Host and path.
   `AB_S3_ENDPOINT=http://garage:3900` is used internally. Neither origin accepts
   a path prefix. Keep the Garage RPC and admin interfaces private.
5. Create a dedicated signing home outside the checkout (mode 0700), generate or
   import a signing key there, and set `AB_REPO_GPG_KEY` to its full fingerprint.
   Copy that home to `./repo-gnupg` (excluded from Git and Docker contexts). The
   repo container copies the read-only source into a writable temporary home.
   Use a dedicated unattended key without an interactive passphrase prompt.
   Never mount this directory into a worker. Back it up securely.
6. Set nonempty `AB_REPO_USER` and `AB_REPO_PASSWORD`. Start server first to run
   migrations, then start repo, web, notifier, and the rebuilt worker/builder
   images. Put the repo endpoint (port 3001) behind an HTTPS reverse proxy.
   Run one repo instance; ingestion deliberately has a single serialized writer.

Credentials in this Compose example are shared between server and repo. Production
installations can give the server permission only to write build objects, and repo
permission to read build objects and read/write published objects using separate
service environments. Avoid mounting the shared `.env` into workers.

## Pacman clients

Export the signing public key and distribute it through a trusted channel. On each
client use `pacman-key --add public-key.asc` followed by
`pacman-key --lsign-key FULL_FINGERPRINT`. Add:

```ini
[aur-builder]
SigLevel = Required DatabaseRequired
Server = https://USER:PASSWORD@repo.example.com/aur-builder/os/$arch
```

Percent-encode reserved characters in credentials and restrict access to this
configuration. Run `pacman -Syu`, or `pacman -Fy` for file databases. Packages with
architecture `any` are included in the x86_64 repository. Other architectures are
rejected in this version. Unsigned or untrusted artifacts must fail verification.
Before the first successful publication the repository returns 404 for databases.

## Rollout and recovery

This release changes the task wire format and makes RabbitMQ queues durable.
Stop producers and workers, finish outstanding builds, then remove the old empty
`pkg_build`, `build_results`, `notifications`, and `repo_ingest` queues before deploying the new
services. Queue durability cannot be changed in place. Rebuild and deploy the
build-container image alongside workers and set `AB_BUILDER_TAG` accordingly.
Existing Gitea packages are not imported; force rebuilds through the existing web
interface to populate this repository.

Upload grants last 24 hours by default (`AB_UPLOAD_TTL`, max seven days). Workers
return expired queued tasks through `expired_tasks`; the server validates them and
issues a new attempt. A build that runs beyond its grant fails upload and must be
rebuilt. Configure the lifetime above expected queue time plus build duration.

The database persists issued attempts and the authoritative published catalog.
The repo acknowledges ingestion only after uploading packages, detached signatures,
and both signed databases, then atomically storing the new catalog. On failure the
previous catalog stays available; ingestion retries every ten seconds. Correct the
underlying signing/storage failure to let the queue recover. Redelivery after a
successful catalog commit is harmless. Invalid bundles and malformed JSON go to the durable `repo_ingest_failed`
queue for operator inspection; transient storage or signing failures retry. Monitor `repo_ingest` depth and repo logs.

Old published objects remain private and retained so clients using an older
snapshot can finish downloads. No automatic garbage collection is performed. Published package filenames are
immutable: rebuilding different bytes under the same filename is rejected; bump
`pkgrel` (or `pkgver`) before rebuilding changed packages.
Back up the SQL database, Garage state, and signing key. Restore all three before
starting writers. A lost signing key requires distributing and trusting a new key.
`AB_REPO_MAX_BYTES` bounds bundle download and extracted package bytes (default
4 GiB); package uploads and downloads stream through bounded buffers. Keep package build logs in the existing SQL/UI
flow; bundles contain packages only.

## Integration test

With a disposable Garage bucket, signing home and test key configured through the
same `AB_S3_*`, `GNUPGHOME` and `AB_REPO_GPG_KEY` variables, run:

```sh
cargo test -p repo private_signed_publication -- --ignored --nocapture
```

This writes artifacts to that bucket. It requires `gpg`, `bsdtar`, `repo-add`,
`pacman`, and `fakeroot` (the latter lets pacman operate on temporary directories
without root). It checks anonymous access denial, signed PUT/GET/HEAD and expiry,
publication, detached signatures, authenticated redirects, range requests, split packages, failed publication,
and pacman installation into a temporary root.

The repo ingestion queue has a dead-letter routing policy. If upgrading an earlier
version of this implementation, drain and recreate `repo_ingest` before restarting
producers; RabbitMQ cannot change queue arguments in place. Background consumer
failures restart the service, allowing unacknowledged deliveries to be recovered.

To verify publisher confirmations with a disposable RabbitMQ instance, set
`AMQP_ADDR` and run `cargo test -p common routed_confirmations -- --ignored`.
The test verifies that mandatory unroutable publications are rejected.
