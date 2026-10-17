# One-container Diktiax for Render's free web service: FalkorDB (Redis + module) and the
# backend in the same image. The graph lives in memory and is re-seeded whenever the
# container starts, so a sleep/restart always comes back to a complete demo graph.
# Build context is the REPOSITORY ROOT:  docker build -f deploy/render/Dockerfile .

FROM rust:1-slim-bookworm AS builder
WORKDIR /app/backend
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN mkdir src && echo "" > src/lib.rs && echo "fn main() {}" > src/main.rs \
    && cargo build --release && rm -rf src
COPY backend/src ./src
COPY data /app/data
RUN touch src/lib.rs src/main.rs && cargo build --release

FROM falkordb/falkordb:v4.22.1
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/backend/target/release/claimgraph-backend /usr/local/bin/diktiax-backend
COPY deploy/render/start.sh /usr/local/bin/start.sh
RUN chmod +x /usr/local/bin/start.sh
ENV HOST=0.0.0.0 PORT=10000 FALKORDB_URL=falkor://127.0.0.1:6379 FALKORDB_GRAPH=claimgraph
# Unprivileged: redis and the backend both run as uid 10001; redis writes only to /tmp.
USER 10001:10001
EXPOSE 10000
ENTRYPOINT ["/usr/local/bin/start.sh"]
