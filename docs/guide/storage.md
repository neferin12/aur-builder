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

Before Compose commands, generate the service env files and host inventory as
shown in [manual setup](./getting-started.md#build-and-start-services). Then start
Garage and inspect its node ID:

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
Rerun `python3 scripts/provision_broker.py` after saving credentials to refresh the
service env files. Keep `AB_S3_BUCKET=aur-builder` and `AB_S3_REGION=garage` unless you intentionally
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

Trusted application services receive the configured storage credentials in their
separate service environment files. The worker writes completed build bundles;
repo reads them and publishes signed objects. Build containers receive no storage
credentials or upload capability. The receipt key is shared only by worker and repo.

Upload and download URLs expire after 15 minutes by default. Anyone with a signed
URL can use it until expiry. Do not log signed query strings.
Set `AB_UPLOAD_TTL` between 60 and 900 seconds. The trusted worker issues the grant
after the build finishes, so queue wait and build time do not consume its lifetime.
Build containers receive no upload capability. Ingestion requires the worker receipt
and verifies the bundle digest before signing.

## Persistent state

Compose uses `garage-meta` and `garage-data` volumes. Back up both together with
the SQL catalog and GPG key. Old published package objects remain private and are
retained so earlier database snapshots can still resolve their package downloads.
Automatic garbage collection is not implemented.

For Garage-specific administration, consult the
[Garage documentation](https://garagehq.deuxfleurs.fr/documentation/quick-start/).
