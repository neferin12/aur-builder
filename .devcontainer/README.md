# Development container

Open the repository in an editor supporting Dev Containers and choose **Reopen in
Container**. Docker with Compose is required on the host. The container includes
Node.js 24, stable Rust, Docker CLI access, native build dependencies, and editor
extensions. Creation installs npm dependencies, migrates isolated databases,
provisions Garage, waits for Keycloak, and seeds application permissions.

Inside the container:

```sh
python3 .devcontainer/run.py web       # Nuxt with hot reload
# In separate terminals, when needed:
python3 .devcontainer/run.py server    # scheduler
python3 .devcontainer/run.py notifier  # deliver notifications to Mailpit
python3 .devcontainer/run.py test      # SQLite + disposable PostgreSQL tests
python3 .devcontainer/run.py schema-check
```

The scheduler configuration starts with no packages, so opening the workspace
never starts real builds. Add test packages in `.devcontainer/config.yaml` when
needed. Workers are opt-in (`python3 .devcontainer/run.py worker`) and use the host
Docker daemon; build containers still need network access to the configured S3
upload endpoint and must trust its development CA when using local HTTPS. Full Arch build/repository signing workflows retain the setup
requirements described in the main documentation.

## Local URLs and credentials

| Service | URL | Login |
| --- | --- | --- |
| AUR Builder | https://aur.localhost:8443 | App test users below |
| Keycloak admin | https://auth.localhost:8443/admin/ | `keycloak-admin` / `dev-keycloak-password` |
| Mailpit inbox | https://mail.localhost:8443 or http://localhost:8025 | None |
| RabbitMQ management | http://localhost:15672 | `builder` / `dev-rabbitmq-password` |
| S3 endpoint | https://s3.localhost:8443 | Preconfigured in `dev.env` |

All app users have password **`dev-password`**:

| Username | Initial application access |
| --- | --- |
| `admin` | Administrator through the `aur-builder-admins` OIDC group |
| `reviewer` | View, approve/reject, rebuild |
| `viewer` | View only |
| `pending` | Signed in, no viewing permission |

The realm import configures the confidential `aur-builder` client, its exact
callback URL, PKCE S256, and the groups claim in signed ID tokens. Non-admin
permissions live in the application database, just as in production. Repeated
bootstrap preserves changes you make to those permissions.

Mailpit accepts authenticated **implicit TLS on port 465**, matching the existing
Rust notifier, with `builder` / `dev-mail-password`. Keycloak also sends its test
emails there. Inside the Compose network use port 465; host tools can use `localhost:1465`
with the development CA trusted. The public
inbox captures mail locally and does not relay it.

## Trust local HTTPS once

The stack generates a development CA and a certificate for the local hostnames.
The container trusts the CA automatically, including Node's OIDC requests and
Keycloak's SMTP client. Production HTTPS, cookie, token-signature, and CSRF checks
remain enabled.

After creation, import `.devcontainer/state/ca.crt` into your **host/browser** trust
store as a certificate authority. This manual host step is necessary because a
container cannot safely modify your browser's trust store. `.localhost` names
resolve to loopback in modern browsers; if your host resolver does not support
them, add this hosts entry:

```text
127.0.0.1 aur.localhost auth.localhost mail.localhost s3.localhost
```

The same names resolve to the HTTPS proxy inside Docker, so the browser and Nuxt
use the same OIDC issuer. Open the URLs above directly; editor-generated tunnel
URLs change the OIDC origin and are not configured redirect URIs. This setup is
intended for local Docker development, not remote Codespaces.

## Isolation, persistence, and reset

All published ports bind to host loopback. Credentials are public test values in
`dev.env`, never deployment secrets. Commands load that file into the environment
and never overwrite the root `.env` or `config.yaml`. SQLite databases and the
public CA export live in ignored `.devcontainer/state/`; service data and the CA
persist in Compose volumes under the `aur-builder-dev` project.

Reapply setup after pulling new migrations or changing the realm configuration:

```sh
python3 .devcontainer/run.py bootstrap
```

Keycloak imports a realm only when it does not already exist. To reimport changed
realm fixtures, remove only this stack's Keycloak volume (after stopping the
stack). To reset **all development service data**, run from the host:

```sh
docker compose -f .devcontainer/compose.yaml down --volumes
```

Remove `.devcontainer/state/` separately to reset app users and permissions.
Removing the certificate volume creates a new CA; import the new CA on the host
after recreation. The PostgreSQL integration-test database is disposable and is
separate from both the app database and the schema-drift fixture database.

## Verify the dependency stack from the host

```sh
docker compose -f .devcontainer/compose.yaml up -d --build certificates rabbitmq keycloak mailpit garage postgres proxy
python3 .devcontainer/smoke.py
```

The smoke check verifies HTTPS certificates and issuer URLs, imported users,
RabbitMQ authentication, repeatable Garage provisioning, and a real authenticated
SMTP delivery captured by Mailpit. The same check runs in CI.
