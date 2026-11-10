FROM ghcr.io/blackdex/rust-musl:x86_64-musl AS chef
USER root
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json --target x86_64-unknown-linux-musl
# Build application
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl && cargo build --release -p database --example migrate --target x86_64-unknown-linux-musl

FROM alpine AS server
RUN apk add --no-cache ca-certificates
WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/server /usr/local/bin/server
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/examples/migrate /usr/local/bin/migrate

CMD ["/usr/local/bin/server"]

FROM node:24-bookworm-slim AS web-base
RUN apt-get update && apt-get install -y --no-install-recommends openssl ca-certificates && rm -rf /var/lib/apt/lists/*

FROM web-base AS web-builder
RUN apt-get update && apt-get install -y --no-install-recommends python3 make g++ && rm -rf /var/lib/apt/lists/*
WORKDIR /app/web
COPY web/package.json web/package-lock.json ./
COPY web/prisma ./prisma
COPY web/scripts ./scripts
COPY web/nuxt.config.ts web/theme.ts web/tsconfig.json ./
RUN npm ci
COPY web .
RUN npm run build

FROM web-base AS web
WORKDIR /app
ENV NODE_ENV=production HOST=0.0.0.0 PORT=3000
COPY --from=web-builder /app/web/.output ./.output
EXPOSE 3000
CMD ["node", ".output/server/index.mjs"]

FROM docker AS worker
WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/worker /usr/local/bin/worker

CMD ["/usr/local/bin/worker"]

FROM alpine AS notifier
RUN apk add --no-cache libssl3 ca-certificates
WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/notifier /usr/local/bin/notifier
COPY notifier/src/templates /app/notifier/src/templates

CMD ["/usr/local/bin/notifier"]
FROM archlinux:base AS repo
RUN pacman -Syu --noconfirm --needed pacman gnupg libarchive ca-certificates && rm -rf /var/cache/pacman/pkg/*
WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/repo /usr/local/bin/repo
CMD ["/usr/local/bin/repo"]
