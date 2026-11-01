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
building. Deploy updated builder images when changing the Rust builder.

## Implementation and validation

The standalone Rust crate uses Paru's underlying `aur-depends`, `aur-fetch`,
`raur`, `srcinfo`, and libalpm libraries. It is excluded from the main workspace
because it requires Arch Linux's libalpm. The Docker build compiles and tests it
in an Arch stage; the runtime image contains neither yay nor the Rust toolchain.

On an Arch host with Rust and base-devel installed:

```sh
cargo test --manifest-path build-container/Cargo.toml --locked
cargo clippy --manifest-path build-container/Cargo.toml --locked -- -D warnings
```

The builder updates system packages, resolves dependencies against pacman's
configured repositories and AUR, then builds AUR dependencies before the target.
Version constraints, split packages, provides, build dependencies, and native
architecture fields are resolved from package metadata. Target checks follow
makepkg defaults and `AB_OPTIONS`; dependency checks and their check-only
dependencies are always skipped. Build dependencies remain installed until the
container is discarded. Ambiguous AUR providers, unresolved dependencies,
conflicts, and blocked dependency cycles fail without prompting. AUR metadata
that changed between RPC resolution and checkout can also produce a blocked
build; the error lists missing dependencies.

`AB_OPTIONS` is split on ASCII whitespace into literal arguments. Quotes and
wildcards receive no shell interpretation. Child commands read from a closed
stdin, so unexpected prompts cannot hang a build. Package archives are selected
with `makepkg --packagelist`; signatures and absent optional debug archives are
excluded. Both build output streams
are copied into the build log and container logs.

## Container cleanup

The worker collects final logs, then removes the build container and its anonymous
volumes after successful or failed builds. Cleanup is also attempted when startup
fails or the Docker wait stream ends unexpectedly; force removal stops a container
that may still be running. Cleanup failures are logged by the worker. Automatic
removal on exit is disabled so logs remain available until collection finishes.

This cleanup requires the worker and Docker daemon to remain available. A worker
crash or an unavailable daemon can leave a container behind. Build logs remain in
the recorded build result, and uploaded bundles remain in Garage.

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
| 104 | Metadata, dependency resolution, or installation failed |
| 105 | Package build failed |
| 106 | Unable to collect outputs or create the bundle |
| 107 | Bundle upload failed |

A missing upload URL fails with code 107 before any build starts. Packages are
signed by the repo service after upload and validation.
