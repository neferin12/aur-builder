# Build container

Arch Linux container used by workers to build a package and upload its outputs as
a tar bundle to a private Garage object using the task's presigned PUT URL.

For full system setup, see the [project README](../README.md) and
[storage documentation](../STORAGE.md).

## Building

From the workspace root:

```sh
docker build -t your-registry/aur-builder-build-container:your-tag build-container
```

Publish the image somewhere your worker Docker hosts can pull it. Configure workers
with the matching builder image and tag (`builder` and `builder_tag` in a worker
configuration file selected with `AB_CONFIG_PATH`). Workers pull the image before
building. Deploy updated builder images when changing these scripts.

## Environment variables

The worker supplies these variables for each build:

| Name | Description |
| --- | --- |
| `AB_SOURCE` | Git URL containing the package source and `PKGBUILD` |
| `AB_SUBFOLDER` | Optional package directory inside the checkout |
| `AB_OPTIONS` | Optional arguments passed to `makepkg` |
| `AB_UPLOAD_URL` | Presigned PUT URL for the build's bundle object |

Per-package environment variables configured on the server are also passed into
the container. S3 credentials and the repository's GPG key are not needed here.
Treat upload URLs as secrets and avoid printing them in build output.

## Exit codes

| Code | Description |
| --- | --- |
| 100 | Unable to change directory |
| 101 | Source environment variable missing |
| 102 | Git clone failed |
| 103 | Package update failed |
| 104 | Dependency installation failed |
| 105 | Package build failed |
| 106 | Unable to collect outputs or create the bundle |
| 107 | Bundle upload failed |

Other shell or tool failures can also produce nonzero exit codes. A missing upload
URL fails before any build starts. Packages are signed by the repo service after
upload and validation.
