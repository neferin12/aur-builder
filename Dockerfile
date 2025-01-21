FROM clux/muslrust:stable AS chef
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
RUN cargo build --release --target x86_64-unknown-linux-musl

FROM alpine AS server
WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/server /usr/local/bin/server

CMD ["/usr/local/bin/server"]

FROM alpine AS web
WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/web /usr/local/bin/web
COPY src/web/templates /app/src/web/templates

CMD ["/usr/local/bin/web"]

FROM docker AS worker
WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/worker /usr/local/bin/worker

CMD ["/usr/local/bin/worker"]