FROM archlinux AS base
LABEL authors="neferin12"
RUN pacman -Syu --noconfirm --needed archlinux-keyring base-devel git openssl ca-certificates

FROM base AS proot-compile
RUN pacman -S --noconfirm --needed talloc pkgconf
RUN git clone https://github.com/proot-me/proot.git /src && git -C /src checkout 25dc6a3134891f98a79f57ce1c2c1b23ff15cad1 && make -C /src/src

FROM base AS compile
RUN pacman -S --noconfirm --needed rust
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo test --locked && cargo build --release --locked

FROM base AS seed
RUN pacman -S --noconfirm --needed talloc procps-ng distcc python mercurial bzr subversion openssh wget yarn nano curl && rm -rf /var/cache/pacman/pkg/*
RUN useradd -u 1000 -m -d /build -s /bin/bash builder
COPY --from=compile /src/target/release/build-package /usr/bin/build-package
COPY --from=proot-compile /src/src/proot /usr/bin/proot
RUN mkdir /results && chown builder:builder /results
USER builder
WORKDIR /build
RUN git config --global init.defaultBranch main && mkdir -p /build/.config/pacman /build/.gnupg /build/out && echo 'keyserver-options auto-key-retrieve' > /build/.gnupg/gpg.conf
USER root
RUN mkdir -p /sandbox /opt && tar --exclude=var/cache/pacman/pkg --exclude="dev/*" --exclude="proc/*" --exclude="tmp/*" --exclude="run/*" -C / -cpf /opt/arch-rootfs.tar usr etc var bin sbin lib lib64 build results dev proc tmp run root
COPY entrypoint.sh /usr/local/bin/build-entrypoint
RUN chmod 755 /usr/local/bin/build-entrypoint
USER 1000:1000
ENV HOME=/build PROOT_NO_SECCOMP=1
ENTRYPOINT ["/usr/local/bin/build-entrypoint"]
