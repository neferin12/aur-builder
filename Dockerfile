FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release

FROM alpine AS server
WORKDIR /app
COPY --from=builder /app/target/release/server /app

CMD ["/app/server"]

FROM docker AS worker
WORKDIR /app
COPY --from=builder /app/target/release/worker /app

CMD ["/app/worker"]