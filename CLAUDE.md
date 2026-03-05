# AgentDNS

Self-hosted DNS + deploy platform for instant preview deployments and demos.
Designed for AI agents to configure DNS and spin up live branch demos autonomously.

## Stack
- **Agent API:** Rust + Axum + Bollard (Docker) + SQLite
- **MCP Server:** Rust stdio MCP server for AI agent integration
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
- Plan/Apply pattern for safe agent-driven deployments
- Protected DNS records (NS, SOA, CAA cannot be modified via API)
- Audit log on all mutations for traceability

## API Endpoints
```
# Public
GET  /api/health              System health
GET  /api/tls-check            Caddy TLS validation

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
POST /api/records              Create record (NS/SOA/CAA blocked)
GET  /api/records              List records
DEL  /api/records/{name}       Delete record

# Audit
GET  /api/audit                View audit log

# Webhooks
POST /api/webhook/github       GitHub push webhook (auto-deploy)
```

## Development
```bash
# API
cd agent-api && cargo run

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
    "agentdns": {
      "command": "/path/to/agentdns-mcp",
      "env": {
        "AGENTDNS_URL": "http://your-server:8053",
        "AGENTDNS_API_KEY": "your-api-key"
      }
    }
  }
}
```

MCP Tools available: `deploy_preview`, `list_deployments`, `get_deployment`,
`teardown`, `get_logs`, `create_dns_record`, `list_dns_records`,
`delete_dns_record`, `health`, `promote`, `plan_deploy`, `apply_plan`

## Project Layout
- `agent-api/` — Rust Axum HTTP service (the brain)
- `cli/` — Rust CLI tool (`agentdns`)
- `mcp-server/` — MCP server for AI agent integration (stdio transport)
- `coredns/` — CoreDNS config and zone files
- `caddy/` — Caddyfile
- `scripts/` — Setup and install scripts
- `PLAN.md` — Full architecture and implementation plan
