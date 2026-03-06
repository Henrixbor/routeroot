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
- **Multi-domain** — Serve multiple domains simultaneously (e.g. `routeroot.dev` + `vibeyard.io`)
- **Auto-detection** — Node.js, Rust, Go, Python, static sites, or bring your own Dockerfile
- **Plan/Apply** — Dry-run deployments before executing (safe for agents)
- **Promote** — Move preview → staging → production
- **Custom Domains** — Map `client.com` to any deployment
- **Path Routing** — Deploy at `yourdomain.dev/client/staging` instead of subdomains
- **MCP Server** — 19 tools for Claude Code / any MCP client
- **CLI** — `routeroot deploy`, `ls`, `logs`, `down`, `promote`, `audit`, `setup`
- **GitHub Webhooks** — Auto-deploy on push, auto-teardown on branch delete
- **DNS Management** — Create/delete DNS records via API (NS/SOA/CAA protected)
- **Audit Log** — Every mutation logged with actor, action, timestamp
- **Verification** — DNS + HTTP health checks after every deployment
- **Auto-expire** — Preview deployments auto-cleanup after configurable TTL
- **On-demand TLS** — Automatic HTTPS via Let's Encrypt for every subdomain
- **Security hardened** — Constant-time auth, repo allowlist, zone injection prevention, CORS, internal network isolation

## Quick Start

### 1. Get a domain + server

- Buy a cheap domain (e.g. `routeroot.dev`, `mypreview.sh`)
- Get a server (Hetzner CX22 for $5/mo, or any VPS with Docker)
- At your registrar, set nameservers: `ns1.yourdomain.dev` → your server IP

### 2. One-command setup

```bash
# On your server (Ubuntu/Debian):
git clone https://github.com/Vibeyard/AgentDNS.git routeroot
cd routeroot
sudo bash scripts/setup.sh
```

That's it. The script will:
- Install Docker if needed
- Ask for your domain and detect your server IP
- Generate a secure API key (min 16 chars, rejects insecure defaults)
- Create zone files for your domain(s)
- Build and start all services
- Install a systemd service (auto-start on reboot)
- Install a watchdog cron (self-healing every 2 minutes)
- Print your API key and full MCP/CLI setup instructions
- Configure the MCP server for Claude Code integration

After install, configure DNS at your registrar:
1. Set custom nameservers: `ns1.yourdomain` and `ns2.yourdomain`
2. Create glue records pointing both to your server IP
3. Verify the zone file (`coredns/zones/db.yourdomain`) has A records pointing to your actual server IP (not `127.0.0.1`)

The setup script prints registrar-specific instructions for Namecheap, Porkbun, etc.

### Manual setup (if you prefer)

```bash
cp .env.example .env
# Edit .env: set ROUTEROOT_DOMAIN, ROUTEROOT_SERVER_IP, ROUTEROOT_API_KEY (min 16 chars)
# Optional: ROUTEROOT_DOMAINS=domain1.com,domain2.com for multi-domain

# Generate zone file for your domain (setup.sh does this automatically)
mkdir -p coredns/zones data
# Create coredns/zones/db.yourdomain with A records pointing to your server IP

docker compose up -d
# API is only accessible via HTTPS through Caddy:
curl https://api.yourdomain.dev/api/health
```

Note: The agent-api container needs Docker access (socket is mounted) and includes `docker-ce-cli` for builds.
Caddy needs `host.docker.internal` resolution to reach deployment containers (configured via `extra_hosts` in docker-compose.yml).
The API port (8053) is bound to localhost only — external access goes through Caddy at `https://api.yourdomain`.

### 3. Deploy your first branch

```bash
# Install CLI
cargo install --path cli

# Deploy
export ROUTEROOT_URL=https://api.yourdomain.dev
export ROUTEROOT_API_KEY=your-key
routeroot deploy https://github.com/user/repo --branch main
```

### 4. Connect AI agents (MCP)

The `setup` command handles end-to-end configuration:

```bash
# Build the MCP server binary
cargo install --path mcp-server

# Show full setup instructions (including MCP config)
routeroot setup
```

Or configure manually — add to `~/.claude/mcp.json`:
```json
{
  "mcpServers": {
    "routeroot": {
      "command": "routeroot-mcp",
      "env": {
        "ROUTEROOT_URL": "https://api.yourdomain.dev",
        "ROUTEROOT_API_KEY": "your-key"
      }
    }
  }
}
```

Restart Claude Code — 19 tools become available. Now Claude Code can deploy branches, check status, read logs, and tear down previews autonomously.

## API Reference

All endpoints except `/api/health` and `/api/tls-check` require `Authorization: Bearer <API_KEY>`.
The API is accessible at `https://api.yourdomain.dev` (routed through Caddy with TLS).

### Deploy

```bash
# Deploy a branch
curl -X POST https://api.yourdomain.dev/api/deploy \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"repo": "https://github.com/user/repo", "branch": "feat/login", "ttl": "24h"}'

# Response:
# {"name": "repo-feat-login", "url": "https://repo-feat-login.example.dev", "status": "building", "environment": "preview"}
```

### Plan/Apply (safe for agents)

```bash
# Create plan (dry-run)
curl -X POST https://api.yourdomain.dev/api/plan \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"repo": "https://github.com/user/repo", "branch": "main"}'

# Response includes plan ID and list of actions that will be taken

# Apply the plan
curl -X POST https://api.yourdomain.dev/api/plan/PLAN_ID/apply \
  -H "Authorization: Bearer $KEY"
```

### Promote

```bash
# Promote preview to staging
curl -X POST https://api.yourdomain.dev/api/deploy/my-app/promote \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"target": "staging"}'

# Promote to production (removes auto-expire)
curl -X POST https://api.yourdomain.dev/api/deploy/my-app/promote \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"target": "production"}'
```

### Deployments

```bash
# List all
curl https://api.yourdomain.dev/api/deployments -H "Authorization: Bearer $KEY"

# Get details
curl https://api.yourdomain.dev/api/deployments/my-app -H "Authorization: Bearer $KEY"

# Get logs
curl https://api.yourdomain.dev/api/deployments/my-app/logs -H "Authorization: Bearer $KEY"

# Tear down
curl -X DELETE https://api.yourdomain.dev/api/deploy/my-app -H "Authorization: Bearer $KEY"
```

### DNS Records

```bash
# Create record (NS, SOA, CAA are blocked — protected)
curl -X POST https://api.yourdomain.dev/api/records \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "api", "record_type": "A", "value": "1.2.3.4"}'

# List records
curl https://api.yourdomain.dev/api/records -H "Authorization: Bearer $KEY"

# Delete record
curl -X DELETE https://api.yourdomain.dev/api/records/api -H "Authorization: Bearer $KEY"
```

### Custom Domains

```bash
# Map a custom domain to a deployment
curl -X POST https://api.yourdomain.dev/api/domains \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"domain": "app.client.com", "deployment_name": "my-app"}'
# Returns CNAME instructions for the domain owner

# List custom domain mappings
curl https://api.yourdomain.dev/api/domains -H "Authorization: Bearer $KEY"

# Remove a custom domain mapping
curl -X DELETE https://api.yourdomain.dev/api/domains/app.client.com -H "Authorization: Bearer $KEY"
```

### Path-based Routing

```bash
# Deploy at yourdomain.dev/client instead of a subdomain
curl -X POST https://api.yourdomain.dev/api/deploy \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"repo": "https://github.com/user/repo", "branch": "main", "path_prefix": "client/staging"}'
# => https://yourdomain.dev/client/staging
```

### Managed Domains (dynamic)

```bash
# Add a new domain dynamically (no server restart needed)
curl -X POST https://api.yourdomain.dev/api/managed-domains \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"domain": "vibeyard.io"}'
# Creates zone file, updates CoreDNS, adds Caddy TLS + routes
# Returns registrar DNS setup instructions

# List all managed domains
curl https://api.yourdomain.dev/api/managed-domains -H "Authorization: Bearer $KEY"

# Remove a dynamically added domain
curl -X DELETE https://api.yourdomain.dev/api/managed-domains/vibeyard.io \
  -H "Authorization: Bearer $KEY"
```

### Audit Log

```bash
curl https://api.yourdomain.dev/api/audit -H "Authorization: Bearer $KEY"
```

### GitHub Webhook (Auto-deploy on push)

When a webhook is configured, pushes auto-deploy branches and branch deletes auto-teardown deployments.

**Option A: Automatic setup via MCP (recommended)**

If Claude Code has RouteRoot MCP configured, it can set up the webhook automatically:
```
# Claude Code will call: setup_github_webhook(repo="owner/repo", github_token="ghp_...")
# This creates the webhook via GitHub API — no manual steps needed.
```

The MCP tool needs a GitHub personal access token with `admin:repo_hook` permission. If the token isn't available, it returns manual instructions instead.

**Option B: Automatic setup via CLI**

```bash
# If you have the GitHub CLI (gh) installed:
WEBHOOK_SECRET=$(openssl rand -hex 20)
gh api repos/OWNER/REPO/hooks --method POST \
  -f name=web -f active=true \
  -f 'events[]=push' \
  -f config[url]=https://api.yourdomain.dev/api/webhook/github \
  -f config[content_type]=json \
  -f config[secret]=$WEBHOOK_SECRET
echo "Set ROUTEROOT_GITHUB_WEBHOOK_SECRET=$WEBHOOK_SECRET on your server"
```

**Option C: Manual setup**

1. Go to `github.com/OWNER/REPO` → Settings → Webhooks → Add webhook
2. Payload URL: `https://api.yourdomain.dev/api/webhook/github`
3. Content type: `application/json`
4. Secret: your `ROUTEROOT_GITHUB_WEBHOOK_SECRET` value
5. Events: Push events
6. Click "Add webhook"

**Important:** The `ROUTEROOT_GITHUB_WEBHOOK_SECRET` on the server must match the secret in the webhook config. Add it to your `.env` and restart:
```bash
echo "ROUTEROOT_GITHUB_WEBHOOK_SECRET=your-secret" >> .env
docker compose up -d
```

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
| `map_custom_domain` | Map a custom domain (e.g. client.com) to a deployment |
| `list_custom_domains` | List all custom domain mappings |
| `delete_custom_domain` | Remove a custom domain mapping |
| `add_managed_domain` | Dynamically add a new domain (DNS + TLS + routes, no restart) |
| `list_managed_domains` | List all managed domains (config + dynamic) |
| `remove_managed_domain` | Remove a dynamically added domain |
| `setup_github_webhook` | Auto-configure GitHub webhook for a repo (or return manual instructions) |

## CLI Reference

```
routeroot deploy <repo> [-b branch] [-n name] [-t ttl] [-e environment] [--path-prefix prefix]
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
routeroot domain map <domain> <deployment>
routeroot domain ls
routeroot domain rm <domain>
routeroot audit [-l limit]
routeroot health
routeroot setup
```

Environment variables: `ROUTEROOT_URL`, `ROUTEROOT_API_KEY`

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `ROUTEROOT_DOMAIN` | `routeroot.dev` | Primary domain |
| `ROUTEROOT_DOMAINS` | (same as DOMAIN) | Comma-separated list of all domains (e.g. `routeroot.dev,vibeyard.io`) |
| `ROUTEROOT_SERVER_IP` | `127.0.0.1` | Server public IP (must match DNS zone A records) |
| `ROUTEROOT_API_KEY` | (required) | API authentication key (min 16 chars, rejects insecure defaults) |
| `ROUTEROOT_MAX_DEPLOYMENTS` | `20` | Max concurrent deployments |
| `ROUTEROOT_DEFAULT_TTL` | `48h` | Default preview expiry |
| `ROUTEROOT_MAX_MEMORY` | `2048` | MB per container |
| `ROUTEROOT_MAX_CPUS` | `2` | CPUs per container |
| `ROUTEROOT_LOG_FORMAT` | (human) | Set to `json` for structured logging |
| `ROUTEROOT_GITHUB_WEBHOOK_SECRET` | (none) | GitHub webhook HMAC secret |
| `ROUTEROOT_ALLOWED_REPO_HOSTS` | `github.com,gitlab.com,bitbucket.org` | Allowed git repo hosts (HTTPS only) |
| `ROUTEROOT_CADDY_ADMIN` | `http://caddy:2019` | Caddy JSON admin API (set by docker-compose) |
| `DATABASE_PATH` | `/data/routeroot.db` | SQLite DB path (set by docker-compose) |
| `ZONE_FILE_DIR` | `/dns-zones` | CoreDNS zone file directory (one file per domain) |

## Multi-Domain Support

RouteRoot can serve multiple domains simultaneously. Domains can be added two ways:

**Option A: Static config (in .env)**
```bash
ROUTEROOT_DOMAINS=routeroot.dev,vibeyard.io
```

**Option B: Dynamic via API (no restart needed)**
```bash
curl -X POST https://api.yourdomain.dev/api/managed-domains \
  -H "Authorization: Bearer $KEY" \
  -d '{"domain": "vibeyard.io"}'
```

Each domain gets:
- Its own CoreDNS zone file (`coredns/zones/db.domain`)
- Caddy wildcard TLS policy (`*.domain`)
- API route at `api.domain`
- Root domain route
- Independent subdomain deployments

The first domain in the list is the primary (used for deployments by default). Each domain needs NS records at its registrar pointing to the server.

## Security

RouteRoot is hardened for real-world use:

- **API key required** — Min 16 chars, rejects known defaults (`dev-key`, `change-me`, etc.)
- **Constant-time auth** — HMAC-based key comparison prevents timing attacks
- **Repo URL allowlist** — Only HTTPS repos from configured hosts (default: github.com, gitlab.com, bitbucket.org)
- **DNS zone injection prevention** — Record names, types, and values validated against metacharacters
- **Protected DNS records** — NS, SOA, CAA cannot be created/deleted via API
- **TLS cert scoping** — Only issues certs for subdomains with active deployments (prevents ACME abuse)
- **API port localhost-only** — Port 8053 bound to `127.0.0.1`; external access via Caddy HTTPS (`api.domain`)
- **Internal Docker network** — Caddy admin API (`:2019`) not exposed outside the internal bridge network
- **CORS restricted** — Only managed domain origins allowed
- **Container hardening** — `no-new-privileges`, PID limits, empty binds, tmpfs `/tmp`, memory/CPU limits
- **Internal error masking** — Real errors logged server-side, generic messages returned to clients
- **Audit log** — All mutations logged with actor, resource, and details
- **Webhook signature verification** — GitHub HMAC-SHA256

## Architecture

```
Internet → CoreDNS (:53)     ← authoritative DNS, wildcard *.domain → server IP
         → Caddy (:443)      ← reverse proxy, on-demand TLS, JSON API config
         → api.domain (HTTPS) ← control plane API (routed through Caddy)
                ↓
           Docker containers  ← one per deployment, bind 0.0.0.0, resource-limited
                ↓
           SQLite             ← deployment state, plans, audit log
```

**Key architectural details:**
- Agent-API runs in Docker, connects to host Docker via mounted `/var/run/docker.sock`
- `docker-ce-cli` is installed in the agent-api container (from official Docker repo) for image builds
- Caddy is configured via JSON API at startup (not just Caddyfile) — see `proxy.rs::init_caddy_config`
- Caddy reaches deployed containers via `host.docker.internal` (`extra_hosts` in docker-compose)
- Deployed containers bind to `0.0.0.0` (not `127.0.0.1`) so Caddy can route to them
- Path-prefix routes are inserted before the root domain catch-all for correct ordering
- DNS zone files must have A records pointing to the actual server IP
- API port 8053 is localhost-only; all external API traffic goes through Caddy HTTPS

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

## Server Requirements

- Linux server with Docker installed
- Ports open: 53 (DNS), 80 (HTTP), 443 (HTTPS)
- A domain with NS records pointing to this server
- ~2GB RAM minimum for the platform itself, plus resources for deployments

Note: Port 8053 (API) is NOT opened externally — it's localhost-only. External API access goes through Caddy at `https://api.yourdomain`.

## License

MIT
