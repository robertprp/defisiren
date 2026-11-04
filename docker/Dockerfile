FROM rust:1.99.1-bookworm AS build
WORKDIR /app
COPY . .
RUN cargo build --release --bin chain-siren
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd --uid 10001 --create-home siren && mkdir /data && chown siren /data
WORKDIR /app
COPY --from=build /app/target/release/chain-siren /usr/local/bin/chain-siren
COPY config /app/config
COPY fixtures /app/fixtures
USER 10001
EXPOSE 8080
ENTRYPOINT ["chain-siren"]
CMD ["--config", "config/offline.toml", "run"]
