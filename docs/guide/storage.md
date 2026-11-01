# Garage storage

Garage provides the S3-compatible object store. Buckets stay private; workers and
clients access individual objects through short-lived presigned URLs.

## Provision with setup

Run `./setup.py` and answer **y** when asked to start Compose services. Setup
initializes a single-node layout, creates a private bucket, creates or reuses an
access key, and saves the credentials in `.env`. Interrupted runs can be resumed.

## Manual provisioning

Set `GARAGE_RPC_SECRET` in `.env` using a generated 32-byte hex value:

```sh
openssl rand -hex 32
```

Start Garage and inspect its node ID:

```sh
docker compose up -d garage
docker compose exec garage /garage status
```

For a fresh single-node layout, replace `NODE_ID` below with the reported ID:

```sh
docker compose exec garage /garage layout assign -z local -c 10G NODE_ID
docker compose exec garage /garage layout apply --version 1
```

Choose capacity appropriate for the host. Do not apply version 1 to an existing
cluster; inspect its current layout and follow Garage's layout procedure instead.

Create a bucket and access key:

```sh
docker compose exec garage /garage bucket create aur-builder
docker compose exec garage /garage key create aur-builder
docker compose exec garage /garage bucket allow --read --write --key aur-builder aur-builder
```

Save the returned key ID and secret in `AB_S3_ACCESS_KEY` and `AB_S3_SECRET_KEY`.
Keep `AB_S3_BUCKET=aur-builder` and `AB_S3_REGION=garage` unless you intentionally
change the corresponding resources. Do not enable website access for the bucket.

## Internal and external endpoints

```dotenv
AB_S3_ENDPOINT=http://garage:3900
AB_S3_PUBLIC_ENDPOINT=https://storage.example.com
```

The server and repo service use the internal Docker-network endpoint. Uploads from
build containers and redirected client downloads use the public endpoint. A
loopback public URL points to the build container or client itself and will fail.

Use a dedicated origin without a URL prefix. Preserve the original Host header,
path, and query string at the [reverse proxy](./nginx.md#garage-s3-api). Keep the
Garage RPC interface private. The supplied `garage.toml` does not configure an
admin HTTP endpoint or website endpoint.

## Access and lifetimes

The example deployment shares storage credentials between server and repo. More
restricted deployments can use separate environments and Garage keys: the server
needs build-object write access; repo needs build-object read access and published
object read/write access. Workers receive neither key.

Upload URLs expire after 24 hours by default; downloads after 15 minutes. Anyone
with a signed URL can use it until expiry. Do not log signed query strings.
Configure upload lifetime above expected queue wait plus build time. A build that
runs beyond its grant fails upload and needs rebuilding.

## Persistent state

Compose uses `garage-meta` and `garage-data` volumes. Back up both together with
the SQL catalog and GPG key. Old published package objects remain private and are
retained so earlier database snapshots can still resolve their package downloads.
Automatic garbage collection is not implemented.

For Garage-specific administration, consult the
[Garage documentation](https://garagehq.deuxfleurs.fr/documentation/quick-start/).
