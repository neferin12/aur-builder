---
layout: home
hero:
  name: AUR Builder
  text: Your packages. Your build infrastructure.
  tagline: Build Arch Linux packages from AUR and Git sources, inspect the results, and publish a private, GPG-signed pacman repository.
  actions:
    - theme: brand
      text: Set up AUR Builder
      link: /guide/getting-started
    - theme: alt
      text: Explore the architecture
      link: /guide/architecture
features:
  - title: Build from your sources
    details: Track AUR packages and Git repositories, with per-package build options and environment variables.
    link: /reference/configuration
  - title: Keep storage private
    details: Workers authenticate completed bundles and upload with short-lived grants. Build sandboxes receive no storage or signing credentials.
    link: /guide/storage
  - title: Publish signed packages
    details: Authenticate pacman clients, sign packages and databases with GPG, and serve temporary download URLs.
    link: /guide/pacman
  - title: Understand every build
    details: Search packages, inspect build history and worker logs, request rebuilds, and recover from failed publication.
    link: /guide/operations
---
