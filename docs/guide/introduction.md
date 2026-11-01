# Introduction

AUR Builder is a self-hosted build system for Arch Linux packages. It checks
configured AUR and Git sources, schedules builds on Docker workers, and publishes
successful outputs to a private pacman repository.

The web dashboard provides package search, latest recorded build results, build
history, searchable logs, and rebuild requests. An optional notifier sends build
results through SMTP.

## Choose your path

- **New deployment:** follow [Getting started](./getting-started.md).
- **Existing v1 deployment:** read [Upgrade to v2](./upgrading.md) before starting new images.
- **Pacman client:** follow [Connect pacman clients](./pacman.md).
- **Contributor:** see [Development & CI](./development.md).

## Deployment model

The supplied Compose configuration targets a single Docker host with a shared
SQLite database and a single Garage node. Workers need access to the Docker daemon.
The repository has one serialized ingestion writer; run one `repo` instance.

Currently, the repository accepts `x86_64` and `any` packages and serves them through
its x86_64 path. Existing Gitea artifacts are not imported automatically.

## Build results and publication

A successful build means the worker completed the build and uploaded its bundle.
The repo service must still validate, sign, and publish the outputs. A successful
build in the dashboard therefore does not guarantee that pacman can download it yet.

Publication failures keep the previous catalog available. Older package objects
are retained, allowing clients with older database snapshots to finish downloads.
There is no automatic garbage collection.

## Privacy and access

Garage objects are private. The repo service authenticates clients before issuing
short-lived signed download redirects. Anyone holding a signed URL can use it
until expiry.

The web dashboard and rebuild endpoint have no built-in authentication. Put them
behind network restrictions or proxy authentication. Use HTTPS for the externally
reachable repo and S3 endpoints. Keep RabbitMQ and Garage administration internal.
