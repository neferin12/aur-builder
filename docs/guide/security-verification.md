# Security hardening verification

The November 2026 hardening keeps the supported single-host deployment. The
worker's Docker socket remains host-equivalent authority: non-root service users
do not remove that authority. For stronger isolation, run builds on a dedicated
host or VM without the repository signing key. PRoot only supplies fake root for
pacman; Docker, its kernel controls, and the isolated network enforce containment.

## Audit coverage

| Finding | Result and verification |
| --- | --- |
| H1: hostile build code | Real UID 1000, no sudo, dropped capabilities, no-new-privileges, explicit seccomp, read-only root, bounded tmpfs/output/logs, memory with no swap, CPU/PID limits, and a four-hour maximum worker deadline. `test_rootless_build.py` exercises official dependencies, locally built AUR-style dependencies and installation hooks. `test_resource_limits.py` exercises fork and memory exhaustion. `test_worker_build.py` verifies the production worker path and a FIFO completion-file timeout. |
| H1: network pivots | Build containers join only an internal bridge with an isolated gateway. The egress proxy permits public HTTP(S), denies private/metadata/host destinations, and has no published port. `test_build_network.py` verifies allowed public access and blocked direct, private, host, and metadata access. |
| H2: public services and broker roles | Broker, management, Garage, and repo ports bind to loopback. `check-compose-security.py` validates rendered Compose configuration in CI. `test_broker_integration.py` verifies per-service queue permissions, including denied cross-role consumption, forged retention, default-exchange writes, bindings, and queue deletion. |
| H3: service privileges | Application images run as UID 10001. The worker receives only its Docker socket group; signing material is staged separately for repo. `test_runtime_permissions.py` checks staging ownership, repeat provisioning, and preservation of the operator's keys. `test_web_container.py` checks the production web user's identity and startup. Docker socket authority remains as described above. |
| M1: administrator revocation | Administrator sessions expire within five minutes; the changed session context invalidates prior sessions. The admin UI exposes session revocation, with audit records. `oidc.test.ts` and `database.test.ts` cover expiry, immediate database/session revocation, permissions, and CSRF. Group removal takes effect on a fresh OIDC login, subject to the provider issuing current claims. |
| M2: artifact capabilities | Neither broker tasks nor sandbox environments receive upload grants, storage credentials, or artifact authentication keys. The trusted worker copies bounded output into its own private file, authenticates its bytes and attempt identity, then requests a short-lived upload. Repo verifies the receipt and downloaded bytes before signing. `test_worker_build.py` checks environment isolation, redaction, and authenticated delivery; `test_publication.py` rejects missing receipts and replaced objects and verifies real GPG signatures and pacman downloads. |
| M3: build logs | Logs are bounded and scrubbed in the worker and again before persistence. UI/API access requires approval permission. Rust redaction tests, web permission tests, and the browser workflow check these paths. Redaction is defense in depth; treat reviewer access to logs as privileged. |
| M4: authentication floods | Login/callback limits run before DB/provider work; only explicitly trusted proxy hops supply client addresses. A timer removes expired login state using an expiry index. `security.test.ts` checks token replenishment and forwarded-address spoofing. Limits are process-local; multiple web replicas need a shared edge limiter. |
| M5: growing queries | Latest build statuses are grouped in SQL. History/review pages have 100-row cursors and exclude logs/snapshots from list projections. Diff output and computation are bounded. Database and security tests cover pagination, both database providers, and dishonest snapshot size metadata. The additive security-index migration supports these queries and expiry cleanup. |
| M6: browser defenses | CSP denies unspecified sources and nonces framework scripts; application page content is never automatically nonced. Permissions Policy and COOP are set. Security tests check nonce boundaries; production HTTPS browser workflows exercise login, navigation, and mutations under the policy. |
| M7: environment injection | Only `AB_PKG_[A-Z0-9_]+` package variables enter the sandbox. The full worker fixture attempts PATH, LD_PRELOAD, and upload URL overrides and checks that they are absent. Existing package variable names need migration. |
| M8: drifted auth | The unused Rust access/session implementation and its module export were removed. Nuxt remains the authentication implementation. |
| L1–L5 | Test TLS keys are generated temporarily. Storage rejects dot segments; AUR names are validated; Git subfolders and metadata symlinks are checked; PostgreSQL opens only its selected Prisma datasource. Unit/database and OIDC tests exercise the relevant paths. |
| L6–L10 | Deployment guidance excludes signed URL/auth query strings from access logs. Operational names are escaped in logs; authentication events are audited; email subjects reject CR/LF; the request-error component displays a generic message. Notification rendering and authentication tests cover these boundaries. |
| L11 | Locked Rust/npm builds, exact Nuxt/Zod/oRPC versions, npm high-severity checks, and enforced OSV scanning are configured. See the dependency decisions below. |
| L12 | AI results are visibly labeled as fallible automated advice. Auto-approval is documented as an explicit supply-chain trust decision; existing baseline, snapshot, and settings guards remain covered by the AI mock tests. |

## Reproduce the checks

Validated on November 12, 2026: locked Rust workspace checks/tests; all five
production application image builds; a clean web install, typecheck and build;
31 web tests across SQLite/PostgreSQL; both production browser workflows; every
integration script listed below; rendered Compose checks; the production web
container smoke test; documentation build; npm's high-severity gate; and strict
OSV scanning with the expiring decisions below.

The integration scripts create temporary keys and disposable containers/networks.
They do not deploy the application or use production signing material.

```sh
cargo check --workspace --locked
cargo test --workspace --locked
python3 -m unittest discover -s scripts -p test_setup.py
python3 -m unittest discover -s scripts -p test_broker_provision.py
python3 scripts/test_broker_integration.py
docker build -t aur-builder-egress-test build-egress
python3 scripts/test_build_network.py
docker build -t aur-builder-rootless-test build-container
python3 scripts/test_rootless_build.py
python3 scripts/test_worker_build.py
python3 scripts/test_resource_limits.py
AB_RUNTIME_INTEGRATION=1 python3 -m unittest discover -s scripts -p test_runtime_permissions.py
AB_PUBLICATION_IN_CONTAINER=1 python3 scripts/test_publication.py
```

Run `.github/workflows/web.yml` for migrations, introspection checks and tests
against both SQLite and disposable PostgreSQL, then the production build, browser
tests, and non-root image smoke test. It requires Node 24 and Docker 28 or newer.
Do not point test database variables at production.

## Dependency applicability decisions

OSV failures remain enabled. The `osv-scanner.toml` files contain advisory-specific
decisions that expire on **December 12, 2026**; reassess them rather than extending
them automatically. The scanner prints every filtered advisory and its rationale.

- Nuxt and its Nitro module are locked to 4.6.4. The [upstream advisory](https://github.com/nuxt/nuxt/security/advisories/GHSA-vjh5-4x84-xrqp)
  says 4.6.2 is patched, despite the scanner's incorrect range match.
- The build-only deepmerge dependency has no upstream patched release.
  `web/scripts/patch-deepmerge.mjs` verifies the exact upstream file hashes and
  rejects prototype keys from both input loops in both distributed modules on
  every normal installation. Security tests exercise both modules. It is absent
  from the production server output; `npm ci --ignore-scripts` omits this protection.
- The builder's raur client explicitly disables HTTP/2, making the
  [h2 empty-frame issue](https://rustsec.org/advisories/RUSTSEC-2026-0258.html)
  unreachable through that client. Upgrade raur when its dependency stack permits.
- The [SQLite/WAL issue](https://rustsec.org/advisories/RUSTSEC-2026-0459.html)
  requires an attacker-written database and sibling WAL. Application DB files are
  operator-created and service-owned, with no untrusted database upload path or
  sandbox mount. Never open DB/WAL files supplied by an untrusted party. Reassess
  a coordinated SQLx/SeaORM upgrade before the exception expires.
- RSA appears only as optional SQLx MySQL lock metadata and is not compiled.
  Verify with `cargo tree --locked --target all -i rsa` after feature changes.
- Backoff, instant, and rustls-pemfile have maintenance-status advisories.
  Replacement requires coordinated upstream SDK/broker/AUR-client upgrades;
  their exceptions do not cover any separate vulnerability advisory.

No claim of an advisory-free dependency tree is made. New advisories and expired
decisions fail the scan. The deployment also retains the ordinary Docker/kernel
and trusted-worker boundaries described above.
