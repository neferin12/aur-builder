#!/usr/bin/env bash
set -euo pipefail
cd /workspaces/aur-builder
sudo cp /dev-certs/ca.crt /usr/local/share/ca-certificates/aur-builder-dev.crt
sudo update-ca-certificates
sudo mkdir -p /usr/local/cargo/registry /usr/local/cargo/git
sudo chown -R "$(id -u):$(id -g)" /usr/local/cargo/registry /usr/local/cargo/git
npm ci --prefix web
python3 .devcontainer/run.py bootstrap
