FROM rust:1.85-slim AS builder
WORKDIR /taba
COPY . .
RUN cargo build --workspace --release --bin taba --bin taba-k8s

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /taba/target/release/taba /usr/local/bin/
COPY --from=builder /taba/target/release/taba-k8s /usr/local/bin/
ENTRYPOINT ["taba"]
