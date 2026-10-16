# Diktiax backend image. Build context is the REPOSITORY ROOT, because
# backend/src/db/seed.rs embeds ../../../data/seed.cypher at compile time:
#   docker build -f backend/Dockerfile -t diktiax-backend .

FROM rust:1-slim-bookworm AS builder
WORKDIR /app/backend

# Dependency layer: compile with stub sources so source edits don't rebuild crates.
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN mkdir src && echo "" > src/lib.rs && echo "fn main() {}" > src/main.rs \
    && cargo build --release && rm -rf src

COPY backend/src ./src
COPY data /app/data
RUN touch src/lib.rs src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home diktiax
COPY --from=builder /app/backend/target/release/claimgraph-backend /usr/local/bin/diktiax-backend

USER diktiax
ENV HOST=0.0.0.0 PORT=3001 FALKORDB_URL=falkor://falkordb:6379 FALKORDB_GRAPH=claimgraph
EXPOSE 3001
ENTRYPOINT ["diktiax-backend"]
