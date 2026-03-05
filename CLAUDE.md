# AgentDNS

Self-hosted DNS + deploy platform for instant preview deployments and demos.

## Stack
- **Agent API:** Rust + Axum + Bollard (Docker) + SQLite
- **DNS:** CoreDNS with file-based zones
- **Proxy:** Caddy with on-demand TLS and admin API
- **CLI:** Rust CLI wrapping the Agent API
- **Orchestration:** docker-compose

## Key Decisions
- Wildcard DNS (`*.domain → server IP`) handles most routing; CoreDNS is lightweight
- Caddy admin API (`:2019`) for dynamic route registration — no config file edits
- Bollard crate for Docker API — no shelling out to `docker` CLI
- SQLite for deployment state — no external DB dependency
- Single Rust binary for the API — minimal deployment footprint

## Development
```bash
# API
cd agent-api && cargo run

# CLI
cd cli && cargo run -- deploy <repo>

# Full stack
docker-compose up
```

## Project Layout
- `agent-api/` — Rust Axum HTTP service (the brain)
- `cli/` — Rust CLI tool
- `coredns/` — CoreDNS config and zone files
- `caddy/` — Caddyfile
- `scripts/` — Setup and install scripts
- `PLAN.md` — Full architecture and implementation plan
