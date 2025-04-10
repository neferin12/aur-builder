FROM archlinux

LABEL authors="neferin12"

RUN pacman --noconfirm -Syyu
RUN pacman --noconfirm -Sy archlinux-keyring

RUN pacman-key --init
RUN pacman-key --populate archlinux

RUN pacman --noconfirm -Syu --needed procps-ng gcc base-devel distcc python python git mercurial bzr subversion openssh wget yarn nano curl
RUN rm -rf /var/cache/pacman/pkg/*

RUN useradd -m -d /build -s /bin/bash builder

RUN echo "builder ALL=(ALL) NOPASSWD: ALL" >> /etc/sudoers

COPY install-dependencies /usr/bin
RUN chmod +x /usr/bin/install-dependencies

COPY build-package /usr/bin
RUN chmod +x /usr/bin/build-package

COPY get-source /usr/bin
RUN chmod +x /usr/bin/get-source

RUN mkdir /results
RUN chown -R builder:builder /results

USER builder

WORKDIR /build

RUN git config --global init.defaultBranch main
RUN mkdir -p /build/{.config/pacman,.gnupg,out}
RUN echo 'keyserver-options auto-key-retrieve' > /build/.gnupg/gpg.conf

RUN git clone https://aur.archlinux.org/yay-bin.git
RUN cd yay-bin && makepkg -sic --noconfirm --noprogressbar
RUN rm -rf yay-bin

CMD ["build-package"]