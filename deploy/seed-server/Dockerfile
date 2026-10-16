# Zeron seed server image. Build from the repository root:
#   docker build -f deploy/seed-server/Dockerfile -t zeron-seed .
# Run with host networking so peers reach the real port (see the docs):
#   docker run -d --name zeron-seed --network host \
#     -v zeron-seed:/var/lib/zeron-seed zeron-seed

FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p seed-server \
 && install -m 0755 target/release/zeron-seed /usr/local/bin/zeron-seed

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --home-dir /var/lib/zeron-seed --create-home --uid 10001 zeron-seed
COPY --from=build /usr/local/bin/zeron-seed /usr/local/bin/zeron-seed
USER zeron-seed
VOLUME ["/var/lib/zeron-seed"]
EXPOSE 51413/tcp 51413/udp
ENV ZERON_SEED_DATA=/var/lib/zeron-seed
HEALTHCHECK --interval=30s --timeout=10s --start-period=20s \
  CMD ["zeron-seed", "health"]
ENTRYPOINT ["zeron-seed"]
CMD ["serve", "--port", "51413", "--health", "127.0.0.1:8571"]
