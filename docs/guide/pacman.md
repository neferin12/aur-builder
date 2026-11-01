# Connect pacman clients

The repository serves signed packages, detached signatures, and signed `.db` and
`.files` databases. Clients first authenticate with repo, then download from
Garage through a temporary signed redirect.

## Trust the signing key

Copy `repo-public-key.asc` from the server through a trusted channel. Verify its
full fingerprint against the administrator's trusted copy, then import and locally
trust it:

```sh
sudo pacman-key --add repo-public-key.asc
sudo pacman-key --lsign-key FULL_FINGERPRINT
```

Only the **public** key belongs on clients. The secret key stays on the server.

## Add the repository

Add this entry to `/etc/pacman.conf`, replacing the hostname and credentials:

```ini
[aur-builder]
SigLevel = Required DatabaseRequired
Server = https://USER:PASSWORD@repo.example.com/aur-builder/os/$arch
```

The section name and URL segment must match `AB_REPO_NAME`. Credentials are
`AB_REPO_USER` and `AB_REPO_PASSWORD`. Percent-encode reserved characters in each
credential and restrict access to files containing them.

Use HTTPS before sending credentials. The S3 hostname also needs a valid HTTPS
endpoint because package downloads are redirected there.

## Install and refresh

```sh
sudo pacman -Syu
sudo pacman -S aur-builder/PACKAGE_NAME
```

To refresh file databases:

```sh
sudo pacman -Fy
```

The repo currently supports `x86_64` clients; `any` packages are included under the
same x86_64 repository path. Before the first successful publication, database
requests return 404.

## Diagnose access

A missing or incorrect login produces 401. A missing catalog entry produces 404.
A successful request returns a redirect to S3. To check authentication without
putting the password in shell history, let curl prompt for it:

```sh
curl --user builder --head https://repo.example.com/aur-builder/os/x86_64/aur-builder.db
```

The redirect contains a temporary credential, so do not post its full Location
header in public logs or issue reports. Signature errors require checking the
trusted fingerprint, key expiry, and publication logs; do not disable signature
verification to work around them.
