FROM archlinux AS base
LABEL authors="neferin12"
RUN pacman -Syu --noconfirm --needed archlinux-keyring base-devel git openssl ca-certificates

FROM base AS compile
RUN pacman -S --noconfirm --needed rust
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo test --locked && cargo build --release --locked

FROM base
RUN pacman -S --noconfirm --needed procps-ng distcc python mercurial bzr subversion openssh wget yarn nano curl && rm -rf /var/cache/pacman/pkg/*
RUN useradd -m -d /build -s /bin/bash builder && echo 'builder ALL=(ALL) NOPASSWD: ALL' >> /etc/sudoers
COPY --from=compile /src/target/release/build-package /usr/bin/build-package
RUN mkdir /results && chown builder:builder /results
USER builder
WORKDIR /build
RUN git config --global init.defaultBranch main && mkdir -p /build/.config/pacman /build/.gnupg /build/out && echo 'keyserver-options auto-key-retrieve' > /build/.gnupg/gpg.conf
CMD ["build-package"]
