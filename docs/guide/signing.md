# GPG signing

The repo service signs packages and pacman database files. It needs a dedicated,
unexpired secret key that can sign unattended. Workers do not need this key.

## Use the setup script

```sh
./setup.py
```

Type **y** at the deployment question. After Garage provisioning, the wizard asks
whether to generate an unattended repository key or import a secret-key export.

- **Generate:** creates an Ed25519 signing key with no passphrase, valid for one year.
- **Import:** asks for a secret-key export file and checks that unattended signing works.
- **Existing key:** reuses keys already in `repo-gnupg/`; if several exist, asks for a fingerprint.

Setup stores the selected full fingerprint in `AB_REPO_GPG_KEY`, verifies signing
with a fresh agent, and exports `repo-public-key.asc` for clients.

::: tip Why the key prompt may not appear
Pressing Enter at `[y/N]` declines deployment and exits before key setup.
`--configure-only` also skips signing. If a key is already present, the script
reuses it without showing the generate/import question.
:::

## Configure a key manually

From the project root:

```sh
mkdir -p repo-gnupg
chmod 700 repo-gnupg

gpg --homedir ./repo-gnupg --batch --pinentry-mode loopback \
  --passphrase '' --quick-generate-key \
  'AUR Builder <builder@example.com>' ed25519 sign 1y

gpg --homedir ./repo-gnupg --list-secret-keys --fingerprint
```

Alternatively, import an existing unattended signing key:

```sh
gpg --homedir ./repo-gnupg --import /path/to/secret-key-export.asc
```

Set the full primary fingerprint in `.env`:

```dotenv
AB_REPO_GPG_KEY=REPLACE_WITH_FULL_FINGERPRINT
```

Export the public key:

```sh
gpg --homedir ./repo-gnupg --armor --export FULL_FINGERPRINT > repo-public-key.asc
gpgconf --homedir ./repo-gnupg --kill gpg-agent
```

Compose copies the source home into a writable temporary home at startup. Recreate
the repo container after updating the fingerprint or key files:

```sh
docker compose up -d --force-recreate repo
```

Configure [client trust](./pacman.md#trust-the-signing-key) before installing packages.

## Renew or rotate

Renew generated keys before their one-year expiry. For example, with the same
unattended key and its full fingerprint:

```sh
gpg --homedir ./repo-gnupg --quick-set-expire FULL_FINGERPRINT 1y
gpg --homedir ./repo-gnupg --armor --export FULL_FINGERPRINT > repo-public-key.asc
gpgconf --homedir ./repo-gnupg --kill gpg-agent
docker compose up -d --force-recreate repo
```

Distribute the updated public key to clients and import it again with `pacman-key
--add`. For a new key, distribute and verify the new fingerprint, update `.env`,
and establish client trust explicitly. Retained packages can still carry signatures
from older signing keys; keep the public keys needed to verify them.

## Protect and back up the key

`repo-gnupg/` is excluded from Git and Docker build contexts. Keep its permissions
restricted and back it up securely. The generated key is unencrypted for unattended
operation, so possession of its files grants signing capability. Never mount this
home into workers or distribute its secret-key export to clients. Run
`python3 scripts/prepare_runtime.py` after key changes: repo reads the generated,
service-owned `repo-signing-runtime` copy, while the operator retains ownership of
the original home. Protect both directories and back up the original.
