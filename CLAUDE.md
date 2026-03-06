# RouteRoot

Self-hosted DNS + deploy platform for instant preview deployments and demos.
Designed for AI agents to configure DNS and spin up live branch demos autonomously.

## Stack
- **Agent API:** Rust + Axum + Bollard (Docker) + SQLite, runs inside Docker
- **MCP Server:** Rust stdio MCP server for AI agent integration
- **DNS:** CoreDNS with file-based zones (multi-domain)
- **Proxy:** Caddy with on-demand TLS, configured via JSON API at startup
- **CLI:** Rust CLI wrapping the Agent API
- **Orchestration:** docker-compose

## Architecture: How Docker-in-Docker Works

Agent-API runs in a Docker container but manages sibling containers on the host:
1. The host Docker socket (`/var/run/docker.sock`) is mounted into the agent-api container
2. Bollard crate connects to this socket for container lifecycle (create, stop, logs, list)
3. `docker-ce-cli` (from official Docker repo) is installed in the agent-api image for `docker build` commands (builder.rs shells out to `docker build`)
4. Deployed containers bind to `0.0.0.0` (not `127.0.0.1`) so Caddy can reach them
5. Caddy uses `host.docker.internal:host-gateway` (set via `extra_hosts` in docker-compose) to reach deployed containers on the host network

## Caddy Configuration

Caddy starts with a Caddyfile for initial bootstrap, but agent-api **replaces the entire config via Caddy's JSON API** at startup (`init_caddy_config` in `proxy.rs`). This is critical because:
- The JSON config sets up on-demand TLS with the validation endpoint (`/api/tls-check`)
- Wildcard cert policies for `*.domain` are configured per managed domain
- The root domain and api.domain routes point to agent-api (for all domains)
- Path-prefix routes are inserted **before** the root domain catch-all (ordering matters)
- Subdomain routes use `host.docker.internal:{port}` to reach deployed containers

## Multi-Domain Support

RouteRoot can serve multiple domains simultaneously (e.g. `routeroot.dev` + `vibeyard.io`):
- Set `ROUTEROOT_DOMAINS=routeroot.dev,vibeyard.io` (comma-separated)
- Each domain gets its own CoreDNS zone file, Caddy wildcard TLS policy, and routes
- The first domain in the list is the primary (used for deployments by default)
- Each domain needs NS records at its registrar pointing to the server

## Security

- **API key required**: min 16 chars, rejects known defaults (`dev-key`, `change-me`, etc.)
- **Constant-time auth**: HMAC-based key comparison prevents timing attacks
- **Repo URL allowlist**: Only HTTPS repos from configured hosts (default: github.com, gitlab.com, bitbucket.org)
- **DNS zone injection prevention**: Record names/types/values validated against metacharacter injection
- **Protected DNS records**: NS, SOA, CAA cannot be created/deleted via API
- **TLS check scoping**: Only issues certs for subdomains with active deployments (prevents ACME abuse)
- **API port localhost-only**: Port 8053 bound to 127.0.0.1; external access via Caddy HTTPS (api.domain)
- **Internal Docker network**: Caddy admin API (`:2019`) not exposed outside the internal network
- **CORS restricted**: Only managed domain origins allowed
- **Audit log**: All mutations logged with actor, resource, and details

## Key Decisions
- Wildcard DNS (`*.domain -> server IP`) handles most routing; CoreDNS is lightweight
- Caddy JSON API (`:2019`) for dynamic route registration — replaces Caddyfile config at startup
- Bollard crate for Docker container lifecycle; `docker` CLI for image builds
- Deployment containers bind to `0.0.0.0` so Caddy can reach them via `host.docker.internal`
- SQLite for deployment state — no external DB dependency
- Single Rust binary for the API — minimal deployment footprint
- Plan/Apply pattern for safe agent-driven deployments
- DNS zone file must have A records pointing to the actual server IP (not 127.0.0.1)

## API Endpoints
```
# Public
GET  /api/health              System health (no server_ip exposed)
GET  /api/tls-check            Caddy TLS validation (deployment-scoped)

# Deploy (auth required)
POST /api/deploy               Deploy a branch directly
DEL  /api/deploy/{name}        Tear down a deployment
POST /api/deploy/{name}/promote Promote to staging/production

# Plan/Apply
POST /api/plan                 Create a deploy plan (dry-run)
POST /api/plan/{id}/apply      Execute a plan
GET  /api/plans                List plans

# Deployments
GET  /api/deployments          List all
GET  /api/deployments/{name}   Get details
GET  /api/deployments/{name}/logs Get logs

# DNS Records
POST /api/records              Create record (NS/SOA/CAA blocked, input validated)
GET  /api/records              List records
DEL  /api/records/{name}       Delete record

# Custom Domains
POST /api/domains              Map custom domain to deployment
GET  /api/domains              List custom domain mappings
DEL  /api/domains/{domain}     Remove custom domain mapping

# Audit
GET  /api/audit                View audit log

# Webhooks
POST /api/webhook/github       GitHub push webhook (auto-deploy)
```

## Development
```bash
# API (requires ROUTEROOT_API_KEY with 16+ chars)
cd agent-api && ROUTEROOT_API_KEY=$(openssl rand -hex 32) cargo run

# CLI
cd cli && cargo run -- deploy <repo>

# MCP Server (stdio)
cd mcp-server && cargo run

# Full stack
docker-compose up
```

## MCP Server Configuration

Add to your Claude Code MCP config (`~/.claude/mcp.json`):
```json
{
  "mcpServers": {
    "routeroot": {
      "command": "/path/to/routeroot-mcp",
      "env": {
        "ROUTEROOT_URL": "https://api.yourdomain.com",
        "ROUTEROOT_API_KEY": "your-api-key"
      }
    }
  }
}
```

MCP Tools available (16):
- `deploy_preview` — deploy a branch (subdomain or path-based)
- `list_deployments`, `get_deployment`, `teardown`, `get_logs`
- `create_dns_record`, `list_dns_records`, `delete_dns_record`
- `health`, `promote`, `plan_deploy`, `apply_plan`
- `map_custom_domain`, `list_custom_domains`, `delete_custom_domain`
- `setup_github_webhook` — auto-configure GitHub webhook (or return manual instructions)

## Docker Compose Key Details
- `agent-api`: mounts `/var/run/docker.sock` (host Docker), `/dns-zones`, `/data`; port 8053 localhost-only
- `caddy`: `extra_hosts: ["host.docker.internal:host-gateway"]`; admin API internal-only
- `coredns`: file-based zones per domain, 5s auto-reload
- All services on `internal` bridge network (Caddy admin not exposed)
- Environment variables flow from `.env` -> `docker-compose.yml` -> containers

## Project Layout
- `agent-api/` — Rust Axum HTTP service (the brain)
  - `src/config.rs` — Config with multi-domain, allowed repo hosts, API key validation
  - `src/auth.rs` — Constant-time HMAC-based API key verification
  - `src/services/proxy.rs` — Caddy JSON API client, including multi-domain `init_caddy_config`
  - `src/services/builder.rs` — `git clone` + `docker build` with repo URL validation
  - `src/services/docker.rs` — Bollard-based container lifecycle
  - `src/services/dns.rs` — Multi-domain zone file writer with injection prevention
  - `src/routes/records.rs` — DNS record CRUD with input validation
  - `src/routes/health.rs` — Health check (no IP leak) + deployment-scoped TLS check
  - `Dockerfile` — installs `docker-ce-cli` from official Docker repo
- `cli/` — Rust CLI tool (`routeroot`)
- `mcp-server/` — MCP server for AI agent integration (stdio transport)
- `coredns/` — CoreDNS config and zone files (one per domain)
- `caddy/` — Caddyfile (bootstrap only; replaced by JSON API at startup)
- `scripts/` — Setup, doctor, and install scripts
- `PLAN.md` — Full architecture and implementation plan
