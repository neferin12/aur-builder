FROM mcr.microsoft.com/devcontainers/typescript-node:24-bookworm
USER root
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config libssl-dev libgit2-dev sqlite3 python3 openssl \
    ca-certificates curl gnupg libarchive-tools \
    && rm -rf /var/lib/apt/lists/*
USER node
