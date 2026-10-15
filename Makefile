.PHONY: help db-up db-down seed run-backend run-frontend triage triage-clean mcp build test test-live docker-full docker-full-down

# Load local settings (git-ignored) and pass them to every recipe's environment.
-include .env
export

help:
	@echo "Diktiax make targets"
	@echo "  db-up             start FalkorDB (docker compose, port 6379)"
	@echo "  db-down           stop it"
	@echo "  seed              load data/seed.cypher (resets the graph)"
	@echo "  run-backend       API server on :3001 (seeds an empty graph on start)"
	@echo "  run-frontend      Next.js dev server on :3000 (bun)"
	@echo "  triage            investigate CLM-48291 (ring) from the CLI; mutates the graph, re-seed after"
	@echo "  triage-clean      investigate CLM-10022 (clean)"
	@echo "  mcp               MCP server on stdio"
	@echo "  build             release backend + production frontend build"
	@echo "  test              unit tests (no database needed)"
	@echo "  test-live         integration tests against a running FalkorDB"
	@echo "  docker-full       FalkorDB + backend + frontend in containers"
	@echo "  docker-full-down  stop that stack"

db-up:
	docker compose up -d falkordb

db-down:
	docker compose down

seed:
	cd backend && cargo run --quiet -- --seed

run-backend:
	cd backend && DIKTIAX_DEV=1 cargo run --release

run-frontend:
	cd frontend && bun install && bun run dev

triage:
	cd backend && cargo run --quiet --release -- --investigate CLM-48291

triage-clean:
	cd backend && cargo run --quiet --release -- --investigate CLM-10022

mcp:
	cd backend && cargo run --quiet --release -- --mcp-server

build:
	cd backend && cargo build --release
	cd frontend && bun install --frozen-lockfile && bun run build

test:
	cd backend && cargo test

test-live:
	cd backend && cargo test -- --ignored

docker-full:
	docker compose -f docker-compose.full.yml up -d --build

docker-full-down:
	docker compose -f docker-compose.full.yml down
