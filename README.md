# RouteRoot

Self-hosted DNS + deploy platform designed for AI agents. Deploy any git branch as a live URL in under 60 seconds.

```bash
# Deploy a branch
routeroot deploy https://github.com/user/repo --branch feat/login
# => https://repo-feat-login.yourdomain.dev

# Or let an AI agent do it via MCP
# Agent calls: deploy_preview(repo="...", branch="feat/login")
# => Returns live URL automatically
```

## What is this?

RouteRoot is a self-hosted alternative to Vercel/Netlify preview deployments, built API-first for AI agents (Claude Code, Cursor, etc.) to autonomously deploy, verify, and manage live branch previews.

**Core loop:** Push code → Agent calls API → Live URL in seconds → Agent verifies → Agent reports back

### Features

- **Instant preview URLs** — Any git branch becomes `https://branch-name.yourdomain.dev`
- **Auto-detection** — Node.js, Rust, Go, Python, static sites, or bring your own Dockerfile
- **Plan/Apply** — Dry-run deployments before executing (safe for agents)
- **Promote** — Move preview → staging → production
- **MCP Server** — 12 tools for Claude Code / any MCP client
- **CLI** — `routeroot deploy`, `ls`, `logs`, `down`, `promote`, `audit`
- **GitHub Webhooks** — Auto-deploy on push, auto-teardown on branch delete
- **DNS Management** — Create/delete DNS records via API (NS/SOA/CAA protected)
- **Audit Log** — Every mutation logged with actor, action, timestamp
- **Verification** — DNS + HTTP health checks after every deployment
- **Auto-expire** — Preview deployments auto-cleanup after configurable TTL
- **On-demand TLS** — Automatic HTTPS via Let's Encrypt for every subdomain

## Quick Start

### 1. Get a domain + server

- Buy a cheap domain (e.g. `routeroot.dev`, `mypreview.sh`)
- Get a server (Hetzner CX22 for $5/mo, or any VPS with Docker)
- At your registrar, set nameservers: `ns1.yourdomain.dev` → your server IP

### 2. One-command setup

```bash
# On your server (Ubuntu/Debian):
git clone https://github.com/Henrixbor/routeroot.git
cd RouteRoot
sudo bash scripts/setup.sh
```

That's it. The script will:
- Install Docker if needed
- Ask for your domain and detect your server IP
- Generate a secure API key
- Create zone files for your domain
- Build and start all services
- Install a systemd service (auto-start on reboot)
- Install a watchdog cron (self-healing every 2 minutes)

### Manual setup (if you prefer)

```bash
cp .env.example .env
# Edit .env: set ROUTEROOT_DOMAIN, ROUTEROOT_SERVER_IP, ROUTEROOT_API_KEY
docker compose up -d
curl http://localhost:8053/api/health
```

### 3. Deploy your first branch

```bash
# Install CLI
cargo install --path cli

# Deploy
export ROUTEROOT_URL=http://your-server:8053
export ROUTEROOT_API_KEY=your-key
routeroot deploy https://github.com/user/repo --branch main
```

### 4. Connect AI agents (MCP)

```bash
# Build the MCP server
cargo install --path mcp-server
```

Add to `~/.claude/mcp.json`:
```json
{
  "mcpServers": {
    "routeroot": {
      "command": "routeroot-mcp",
      "env": {
        "ROUTEROOT_URL": "http://your-server:8053",
        "ROUTEROOT_API_KEY": "your-key"
      }
    }
  }
}
```

Now Claude Code can deploy branches, check status, read logs, and tear down previews autonomously.

## API Reference

All endpoints except `/api/health` require `Authorization: Bearer <API_KEY>`.

### Deploy

```bash
# Deploy a branch
curl -X POST http://server:8053/api/deploy \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"repo": "https://github.com/user/repo", "branch": "feat/login", "ttl": "24h"}'

# Response:
# {"name": "repo-feat-login", "url": "https://repo-feat-login.example.dev", "status": "building", "environment": "preview"}
```

### Plan/Apply (safe for agents)

```bash
# Create plan (dry-run)
curl -X POST http://server:8053/api/plan \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"repo": "https://github.com/user/repo", "branch": "main"}'

# Response includes plan ID and list of actions that will be taken

# Apply the plan
curl -X POST http://server:8053/api/plan/PLAN_ID/apply \
  -H "Authorization: Bearer $KEY"
```

### Promote

```bash
# Promote preview to staging
curl -X POST http://server:8053/api/deploy/my-app/promote \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"target": "staging"}'

# Promote to production (removes auto-expire)
curl -X POST http://server:8053/api/deploy/my-app/promote \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"target": "production"}'
```

### Deployments

```bash
# List all
curl http://server:8053/api/deployments -H "Authorization: Bearer $KEY"

# Get details
curl http://server:8053/api/deployments/my-app -H "Authorization: Bearer $KEY"

# Get logs
curl http://server:8053/api/deployments/my-app/logs -H "Authorization: Bearer $KEY"

# Tear down
curl -X DELETE http://server:8053/api/deploy/my-app -H "Authorization: Bearer $KEY"
```

### DNS Records

```bash
# Create record (NS, SOA, CAA are blocked — protected)
curl -X POST http://server:8053/api/records \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "api", "record_type": "A", "value": "1.2.3.4"}'

# List records
curl http://server:8053/api/records -H "Authorization: Bearer $KEY"

# Delete record
curl -X DELETE http://server:8053/api/records/api -H "Authorization: Bearer $KEY"
```

### Audit Log

```bash
curl http://server:8053/api/audit -H "Authorization: Bearer $KEY"
```

### GitHub Webhook

Set up in your repo: Settings → Webhooks → Add webhook:
- URL: `http://your-server:8053/api/webhook/github`
- Content type: `application/json`
- Secret: your `ROUTEROOT_GITHUB_WEBHOOK_SECRET`
- Events: Push events

Branches auto-deploy on push, auto-teardown on delete.

## MCP Tools

| Tool | Description |
|------|-------------|
| `deploy_preview` | Deploy a git repo branch as a live URL |
| `plan_deploy` | Create a deployment plan (dry-run) |
| `apply_plan` | Execute a plan |
| `list_deployments` | List all active deployments |
| `get_deployment` | Get deployment details |
| `get_logs` | Get container logs |
| `teardown` | Tear down a deployment |
| `promote` | Promote to staging/production |
| `create_dns_record` | Create a DNS record |
| `list_dns_records` | List DNS records |
| `delete_dns_record` | Delete a DNS record |
| `health` | System health check |

## CLI Reference

```
routeroot deploy <repo> [-b branch] [-n name] [-t ttl] [-e environment]
routeroot plan <repo> [-b branch] [-n name] [-t ttl]
routeroot apply <plan_id>
routeroot plans
routeroot promote <name> <target>
routeroot ls
routeroot status <name>
routeroot logs <name>
routeroot down <name>
routeroot record add <name> [-t type] <value>
routeroot record ls
routeroot record rm <name>
routeroot audit [-l limit]
routeroot health
```

Environment variables: `ROUTEROOT_URL`, `ROUTEROOT_API_KEY`

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `ROUTEROOT_DOMAIN` | `routeroot.dev` | Your domain |
| `ROUTEROOT_SERVER_IP` | `127.0.0.1` | Server public IP |
| `ROUTEROOT_API_KEY` | `dev-key` | API authentication key |
| `ROUTEROOT_MAX_DEPLOYMENTS` | `20` | Max concurrent deployments |
| `ROUTEROOT_DEFAULT_TTL` | `48h` | Default preview expiry |
| `ROUTEROOT_MAX_MEMORY` | `2048` | MB per container |
| `ROUTEROOT_MAX_CPUS` | `2` | CPUs per container |
| `ROUTEROOT_LOG_FORMAT` | (human) | Set to `json` for structured logging |
| `ROUTEROOT_GITHUB_WEBHOOK_SECRET` | (none) | GitHub webhook HMAC secret |

## Architecture

```
Internet → CoreDNS (:53)  ← authoritative DNS, wildcard *.domain → server IP
         → Caddy (:443)   ← reverse proxy, on-demand TLS, routes to containers
         → Agent API (:8053) ← control plane, manages everything
                ↓
           Docker containers ← one per deployment, resource-limited
                ↓
           SQLite ← deployment state, plans, audit log
```

## Scaling

### Single server (current — handles 20-50 deployments)
- SQLite: ~50k writes/sec, fine for control plane
- CoreDNS: ~100k queries/sec, fine for DNS
- Bottleneck: Docker containers (RAM/CPU per deployment)

### Multi-server (planned — Phase 3)
- Control plane stays on one server (API + CoreDNS)
- Worker nodes (Hetzner/OVH boxes) run Caddy + Docker
- `routeroot server add --name eu-1 --ip x.x.x.x --region eu`
- API routes deployments to least-loaded worker
- DNS points subdomains to the specific worker IP
- Each $5/mo Hetzner box adds ~15-20 more deployment slots
- Scale linearly by adding boxes

### If it goes viral
1. Spin up Hetzner boxes (API or console, takes 30 seconds each)
2. Run setup script on each
3. Register with control plane
4. New deployments auto-route to available capacity
5. CoreDNS handles the routing — no global DNS propagation delay

## License

MIT
