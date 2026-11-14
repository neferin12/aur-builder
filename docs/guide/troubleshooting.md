# Troubleshooting

## The GPG setup prompt is not shown

At the wizard's `[y/N]` deployment question, pressing Enter chooses No. Type `y`
to proceed to Garage and signing. `--configure-only` intentionally stops before
provisioning. An existing key in `repo-gnupg/` is reused without the generate/import
question. See [GPG signing](./signing.md) for manual setup.

## Repository returns 401

Check `AB_REPO_USER` and `AB_REPO_PASSWORD`, URL-encode credentials in
`pacman.conf`, and make sure the proxy forwards Authorization. Use HTTPS. Avoid an
extra proxy login layer in front of the repo API.

## Repository returns 404

Before the first successful publication, no repository database exists. Confirm
the configured repository name and `/REPO/os/x86_64/FILE` path, then inspect repo
logs. A successful worker build alone does not confirm publication.

## S3 signature or access errors

Check that the public origin matches `AB_S3_PUBLIC_ENDPOINT`. The proxy must keep
the original Host, path, and signed query parameters. Do not use a URL prefix.
Confirm the key belongs to the bucket and has the required permissions, and keep
host clocks synchronized. Confirm that a URL has not expired.

Never include complete signed URLs or storage secrets in shared diagnostic output.

## Upload fails after a long queue wait or build

`AB_UPLOAD_TTL` defaults to 900 seconds and is issued by the worker after the build
finishes. Queue wait and build time no longer consume the grant. Check worker S3
connectivity and the upload deadline rather than increasing the lifetime above 900.
Use matching worker, build-container, and repo images and rerun provisioning after
changing storage settings.

## Worker cannot reach S3

`localhost` inside the worker refers to that worker container. The internal
`AB_S3_ENDPOINT` must be reachable from trusted services; pacman clients use
`AB_S3_PUBLIC_ENDPOINT`. The isolated build sandbox uploads nothing and intentionally
cannot reach Garage directly.
Check DNS, routing, TLS trust, proxy upload limits, and transfer timeouts.

## Signing fails or clients reject signatures

Check the selected full fingerprint, secret-key availability, and expiry. The repo
container starts with a fresh GPG home, so a cached host passphrase cannot provide
unattended signing. After key changes, recreate the repo container and distribute
the updated public key to clients. Keep signature verification enabled.

## Publication rejects an existing filename

Published package filenames cannot change content. Increase `pkgrel` or `pkgver`
when rebuilding changed bytes; do not overwrite immutable objects to work around
this validation. Invalid artifacts appear in `repo_ingest_failed`.

## RabbitMQ reports PRECONDITION_FAILED

An existing queue has different durability or arguments. Stop producers, drain
the old queue, and follow [queue migration](./upgrading.md#queue-compatibility).
Changing default-user environment variables also does not update an existing
broker user's password.

## Builder image cannot be pulled

Check `AB_BUILDER` and `AB_BUILDER_TAG`, registry credentials on the worker's Docker
host, and whether the release image exists. Setup can build a local image. A failed
pull can fall back to a matching local image, but builds need that image to exist.
