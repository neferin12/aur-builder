# Upgrade to v2

V2 replaces direct Gitea uploads with private Garage storage, trusted worker uploads,
authenticated artifacts, and a dedicated signed pacman repository. Treat server, worker, and
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
If your environment overrides `AB_BUILDER_TAG`, ensure that image supports the rootless completion and artifact-export protocol.

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
setup, `repo_ingest`. Run broker provisioning and restart RabbitMQ to load the
new definitions before starting producers. Do not delete queued work unless you
deliberately intend to discard it.

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

## Security hardening migration

Application containers (server/migrations, worker, web, notifier, and repo) now run with
UID/GID `10001:10001`. Before starting them, provision signing keys in `repo-gnupg`,
then prepare runtime ownership:

```sh
python3 scripts/prepare_runtime.py
```

This gives the service UID write access to `db` and `server_tmp` and stages keys in
`repo-signing-runtime`, mounted read-only by repo. It preserves the operator-owned
`repo-gnupg` home so subsequent key provisioning and setup reruns still work. The
staging directory is generated and replaced on reruns; it is not the authoritative
key backup. Back up `repo-gnupg` and protect both directories. Docker applies UID
mapping on rootless engines. The setup wizard performs staging automatically.

RabbitMQ, its management UI, Garage, and the signed repository now publish only
on loopback. Use the documented TLS reverse proxy for remote access. Validate
the final Compose configuration, including overrides:

```sh
docker compose config --format json | python3 scripts/check-compose-security.py
```

Group-derived administrator sessions expire within five minutes. Group removal
is checked at the next OIDC authorization; disable the user or save access in the
admin UI to revoke sessions immediately. The identity provider may retain its
own SSO session and must return current group claims.

Build logs now require approval permission. Existing stored logs are not
retroactively scrubbed; treat database backups and broker volumes as sensitive.
Scrubbing is best effort and cannot recognize transformed or fragmented secrets.
Package environment overrides must use `AB_PKG_` followed by uppercase letters,
digits, or underscores. Update PKGBUILDs to consume the renamed variables.
Other overrides are ignored. This prevents overrides of PATH, HOME, loader,
shell, and Git execution controls.

Build containers now have a host-sized memory ceiling with swap disabled, 512 PID ceiling, four-hour
runtime ceiling, and capped Docker/application logs. Resource ceilings can be adjusted with the documented worker environment settings.
History and review lists fetch at most 100 rows per request; older pages remain
available. Large source files are omitted with an explicit external-review warning.

### Trust boundary and artifact migration

The worker retains host-equivalent Docker socket access in the supported single-host
deployment. Keep the Docker host and worker trusted; a compromised Docker daemon or
worker can still compromise that host. A socket proxy allowing arbitrary container
creation would not remove this boundary. Separate build VMs or a sandbox runtime
remain stronger deployment options.

Bundle delivery now requires a coordinated worker, build-container, and repo
upgrade. Drain running builds first: legacy successful results lack authenticated
receipts and are rejected by ingestion. The build-container writes
`/results/bundle.tar` and an atomic completion record. The worker exports bounded
bytes through a fixed UID-1000 command, freezes the sandbox, then
hashes it, uploads it with a fresh grant lasting at most 900 seconds, and issues
an HMAC receipt bound to the attempt, candidate, revision, object key, size, and
SHA-256 digest. Ingestion verifies the receipt and downloaded bytes before signing.
No upload URL or authentication key enters the build environment or task queue.
`AB_ARTIFACT_KEY` is generated by provisioning and shared only by worker and repo;
protect and back it up. Rotating it invalidates in-flight artifact receipts.
Provisioning caps legacy `AB_UPLOAD_TTL` values at 900 seconds. Builds can run longer
because upload grants are issued after they finish. Authentication success, failure, logout, and explicit session revocation are recorded
in `access_audit`, without codes, tokens, cookies, or provider exception text.


### Broker authorization migration

Stop all producers and consumers before this coordinated upgrade. Keep existing
queues and messages. Generate service credentials, broker definitions, and private
service environment files from `.env`:

```sh
python3 scripts/provision_broker.py
docker compose up -d --force-recreate rabbitmq
# Wait for RabbitMQ to finish importing definitions before restarting applications.
docker compose up -d --force-recreate server worker repo notifier web
```

Run provisioning again after editing `.env`, then recreate affected application
containers. Generated passwords are retained across reruns. Regenerating a service
password requires restarting RabbitMQ to import the new hash and recreating that
service. The generated `.env.*` files contain only that service's selected settings;
do not add the shared `.env` back to application `env_file` entries.

Publishing now uses durable `ab.<queue>` direct exchanges. The administrator imports
the queues, bindings, and permissions at broker startup; applications cannot manage
topology. The legacy administrator's AMQP permissions are explicitly revoked while
management access remains available. Existing unrelated broker users are preserved;
review and revoke any legacy accounts with broad permissions before resuming work.
The setup wizard performs provisioning automatically.

Queue policy rejects publishes once a queue reaches 1,000 messages or 256 MiB.
Confirmed mandatory publishing makes the failure visible to producers. Preserve and
inspect dead-letter messages rather than deleting production queues to clear a limit.
Run `python3 scripts/test_broker_integration.py` on a Docker host to exercise real
AMQP permission boundaries using a disposable broker.


### Build network isolation

Docker Engine 28 or newer is required for isolated gateway mode. Before deployment,
run `python3 scripts/provision_build_network.py` on the Docker host. It records host
interface addresses and resolves the public web/storage reverse-proxy addresses into
`build-egress/host-deny.acl`. It also records the Docker Unix socket path and group so the UID-10001 worker can
use the socket without writing root-owned SQLite journals. Set `DOCKER_SOCKET_PATH`
for a nonstandard or rootless socket before provisioning. Repeat this after changing
host interfaces, socket ownership, or DNS, then
recreate the egress proxy. The setup wizard performs this inventory automatically.

Build containers join only the `aur-builder-build` internal network. Its isolated
gateway has no host interface address. The worker checks this policy before starting
every build and fails closed when the network is missing or incorrectly configured.
Only the egress proxy joins both that network and a separate outbound network.
It denies private, metadata, host, and configured public reverse-proxy addresses.
No application service joins the build network, and the proxy publishes no host port.
Only public HTTP and HTTPS destinations are permitted; sources using SSH, plain Git,
or alternate download ports must migrate to HTTPS. Downloads that ignore proxy
settings fail closed. Keep TLS access logs from recording signed URL query strings;
the build proxy disables URL logging entirely.

To verify with disposable networks and clients:

```sh
docker build -t aur-builder-egress-test build-egress
python3 scripts/test_build_network.py
```


### Rootless build filesystem and resource budgets

The builder now runs with real UID/GID 1000, all capabilities dropped,
no-new-privileges, the vendored Docker seccomp profile, and a read-only outer root.
There is no passwordless sudo grant. A pinned PRoot revision supplies an emulated
root identity only for pacman; official dependencies, AUR dependencies, and their
installation hooks still execute with kernel UID 1000. PRoot is a compatibility
layer, not the security boundary. The outer container restrictions apply even if
untrusted code leaves the emulated filesystem.

Each build extracts an immutable Arch seed into a fresh sized tmpfs. By default,
memory is one quarter of Docker-reported host RAM, capped at 8 GiB per build, and
sandbox storage is three quarters of that memory budget, capped at 6 GiB. This
reserves room for the other default worker replica and host services. Override
with `AB_BUILD_MEMORY_MIB` and `AB_BUILD_DISK_MIB` when needed. The output ceiling is
`AB_BUILD_OUTPUT_MIB=1024`, plus 64 MiB temporary loader storage. Filesystem usage
counts toward the memory ceiling; no swap is allowed. The PID ceiling is 512,
the CPU quota is one core, and `AB_BUILD_TIMEOUT_SECONDS` defaults to 14400 and
cannot exceed four hours. Set resource overrides in `.env`, rerun provisioning,
and recreate workers. Budget for the sum across replicas and sufficient host RAM;
overrides or additional replicas can overcommit the host. Syscall tracing adds
build overhead. Docker must report working memory/swap, PID, CPU, and seccomp
controls before a build is accepted. Custom builder images must implement the
completion/export protocol and declare no persistent volumes; image-declared
anonymous volumes would bypass the sized tmpfs policy and are rejected.

Keep matching worker and build-container versions: completion and output export
now happen before container removal, because stopping discards tmpfs. The worker
never accepts a completion record larger than four bytes and bounds all output
copied into its private temporary directory. It hashes and signs only those copied
bytes; the sandbox cannot modify them afterward. Startup, execution, collection,
and upload errors all force container removal. Timeouts and invalid completion records
produce failed build results, so they do not strand the package as running. Transient
Docker/system errors are requeued with a delay. Finished or superseded attempts
are rejected before execution. Notification messages omit package environment, source
credentials, logs, and artifact receipts.

Verify official dependencies, built dependency packages and their hooks, dependent
makepkg builds, kernel credentials, and writable-space limits:

```sh
docker build -t aur-builder-rootless-test build-container
python3 scripts/test_rootless_build.py
```


### Authentication proxy addresses

By default authentication rate limits use the socket peer and trust no forwarded
headers. Set `AB_TRUSTED_PROXIES` to exact proxy IPs or narrow CIDRs only when those
proxies append or replace `X-Forwarded-For`. The app walks the chain from the closest
hop and uses the first untrusted client address. Malformed or excessive chains fall
back to the socket peer. Do not trust all networks. Rerun provisioning and recreate
web after changing this setting. Authentication limits are process-local; use a
shared edge limiter when deploying multiple web replicas.


### Database and dependency checks

Run the authoritative Rust migrations before starting Nuxt. The new
`m20261111_000001_security_indexes` migration adds indexes for package history,
review pagination, and expired authentication-state cleanup on both SQLite and
PostgreSQL. Nuxt refuses to start until it is present. It preserves existing data.
The Prisma schemas are introspected from those migrations, not maintained as a
separate schema authority.

Nuxt, Zod, and oRPC now declare the exact versions in the lockfile. CI uses locked
installs/builds, runs production npm advisory checks at the high-severity threshold,
and retains repository-wide OSV scanning with failures enabled. At validation time,
the production npm audit reported no high or critical advisories; lower-severity
advisories remain visible in that tool's report.

See [Security hardening verification](./security-verification.md) for audit coverage,
reproducible checks, remaining trust boundaries, and expiring dependency decisions.
