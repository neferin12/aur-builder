#!/bin/bash
set -euo pipefail
# Every runtime process has real UID 1000. PRoot supplies package-manager identity
# emulation only; Docker, capabilities, seccomp and bounded mounts are the boundary.
mkdir -p /sandbox/root
tar --no-same-owner -xf /opt/arch-rootfs.tar -C /sandbox/root
set +e
proot -r /sandbox/root -b /dev -b /proc -b /results -b /etc/resolv.conf -b /etc/hosts \
    -w /build -i 1000:1000 /usr/bin/build-package
status=$?
set -e
printf '%s\n' "$status" > /results/status.tmp
mv /results/status.tmp /results/status
# Keep tmpfs mounted until the worker collects output and freezes the container.
# The worker removes the entire container after collection or any error.
while true; do sleep 3600; done
