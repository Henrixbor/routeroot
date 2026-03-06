# RouteRoot

A self-hosted DNS + deploy platform for instant preview deployments, demos, and API routing.
Built for agentic development workflows where branches become live URLs in seconds.

## Problem

Deploying preview branches, demos, and staging environments requires manual DNS config,
vendor dashboards (Cloudflare, Railway, Vercel), and per-project setup. We want:

- `curl deploy.routeroot/deploy --data '{"repo":"...", "branch":"..."}'` → live URL in 60s
- Zero vendor dependencies for dev/demo infrastructure
- Agentic CI/CD: Claude Code or any agent can deploy and verify branches autonomously

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│  Domain: *.routeroot.dev (or similar)                     │
│  Registrar NS → your server(s)                           │
├─────────────────────────────────────────────────────────┤
│                                                          │
│  ┌──────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │ CoreDNS  │  │  Agent API   │  │  Caddy           │  │
│  │ :53      │  │  :8053       │  │  :80/:443        │  │
│  │ zones/   │◄─│  CRUD DNS    │  │  auto-TLS        │  │
│  │ wildcard │  │  deploy/tear │  │  reverse proxy   │  │
│  └──────────┘  │  logs/status │  │  on-demand certs │  │
│                └──────┬───────┘  └────────▲─────────┘  │
│                       │                    │             │
│                       ▼                    │             │
│                ┌──────────────┐   routes   │             │
│                │  Containers  │────────────┘             │
│                │  per branch  │                          │
│                └──────────────┘                          │
│                                                          │
│  ┌──────────────────────────────────────────────────┐   │
│  │  SQLite (deployments, records, audit log)         │   │
│  └──────────────────────────────────────────────────┘   │
│                                                          │
└─────────────────────────────────────────────────────────┘
```

## Components

### 1. CoreDNS (off-the-shelf)
- Single Go binary, Kubernetes-proven
- Wildcard `*.routeroot.dev → server IP` handles 99% of cases
- File-based zones with 5s auto-reload for custom records
- Plugin: `file` for zone management, `log`, `errors`, `health`

### 2. Agent API (Rust + Axum)
The brain of the system. Single binary that orchestrates everything.
Runs inside Docker but manages sibling containers on the host via mounted Docker socket.

**Endpoints:**
```
POST   /api/deploy              Deploy a branch (clone → build → run → route)
DELETE /api/deploy/:name        Tear down a deployment
POST   /api/deploy/:name/promote  Promote to staging/production
GET    /api/deployments         List active deployments
GET    /api/deployments/:name   Deployment details + status
GET    /api/deployments/:name/logs  Container logs

POST   /api/plan                Create deploy plan (dry-run)
POST   /api/plan/:id/apply      Execute a plan
GET    /api/plans               List plans

POST   /api/records             Create custom DNS record (NS/SOA/CAA blocked)
GET    /api/records             List DNS records
DELETE /api/records/:name       Delete DNS record

POST   /api/domains             Map custom domain to deployment
GET    /api/domains             List custom domain mappings
DELETE /api/domains/:domain     Remove custom domain mapping

GET    /api/health              System health
GET    /api/tls-check           Caddy TLS validation endpoint
GET    /api/audit               Audit log

POST   /api/webhook/github      GitHub push webhook
```

**Deploy flow:**
1. Receive deploy request (repo URL, branch, optional subdomain name or path_prefix)
2. Clone repo, detect build system (Dockerfile / package.json / Cargo.toml / go.mod / Python / static)
3. Build Docker image (via `docker build` CLI in container)
4. Allocate port, start container with resource limits (bind `0.0.0.0`)
5. Register route in Caddy via JSON admin API (using `host.docker.internal:{port}`)
6. Return `https://{name}.domain` or `https://domain/{path_prefix}`
7. Background: verification (DNS + HTTP), auto-expire after TTL

**Tech:**
- Rust + Axum for the HTTP server
- Bollard for Docker container lifecycle (create, stop, logs, list)
- `docker-ce-cli` for image builds (shells out to `docker build`)
- SQLite (via rusqlite) for state
- Tokio for async

### 3. Caddy (off-the-shelf)
- On-demand TLS: auto-provisions Let's Encrypt certs per subdomain
- JSON admin API at `:2019` for dynamic route registration
- **Configured via JSON API at startup** — agent-api replaces Caddyfile config with `init_caddy_config`
- Validation endpoint (`/api/tls-check`): API confirms subdomain is a real deployment before cert issuance
- Requires `extra_hosts: ["host.docker.internal:host-gateway"]` in docker-compose to reach deployed containers
- Routes use `host.docker.internal:{port}` to reach deployment containers on the host network

### 4. CLI (`routeroot`)
Thin Rust CLI that wraps the API.

```bash
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

### 5. MCP Server (`routeroot-mcp`)
Stdio-based MCP server with 15 tools for AI agent integration.
Tools: deploy_preview, list_deployments, get_deployment, teardown, get_logs,
create_dns_record, list_dns_records, delete_dns_record, health, promote,
plan_deploy, apply_plan, map_custom_domain, list_custom_domains, delete_custom_domain.

### 5. GitHub Webhook Handler (in Agent API)
- Listens for push events
- Auto-deploys branches matching configurable patterns
- Auto-tears down on branch delete
- Updates deployment on force-push

## Project Structure

```
RouteRoot/
├── PLAN.md
├── CLAUDE.md
├── .env.example                # All env vars documented
├── docker-compose.yml          # Full stack: CoreDNS + Caddy + Agent API
├── agent-api/                  # Rust Axum service
│   ├── Cargo.toml
│   ├── Dockerfile              # Installs docker-ce-cli from official Docker repo
│   ├── src/
│   │   ├── main.rs             # Startup, Caddy JSON config init
│   │   ├── config.rs           # Env-based config
│   │   ├── routes/
│   │   │   ├── mod.rs
│   │   │   ├── deploy.rs       # Deploy/teardown/promote/plan/apply
│   │   │   ├── records.rs      # DNS record CRUD
│   │   │   ├── domains.rs      # Custom domain mapping
│   │   │   ├── health.rs       # System health + TLS check
│   │   │   └── webhook.rs      # GitHub webhook handler
│   │   ├── services/
│   │   │   ├── mod.rs
│   │   │   ├── docker.rs       # Container lifecycle (bollard)
│   │   │   ├── dns.rs          # Zone file writer + CoreDNS reload
│   │   │   ├── proxy.rs        # Caddy JSON API client + init_caddy_config
│   │   │   ├── builder.rs      # Repo clone + docker build (shells out to CLI)
│   │   │   ├── verify.rs       # DNS + HTTP deployment verification
│   │   │   └── cleanup.rs      # TTL-based reaper task
│   │   ├── db.rs               # SQLite schema + queries
│   │   ├── error.rs            # Error types
│   │   └── auth.rs             # API key middleware
├── cli/                        # Rust CLI
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
├── mcp-server/                 # MCP server for AI agents (stdio transport)
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
├── coredns/
│   ├── Corefile
│   └── zones/
│       └── db.routeroot.dev    # Zone template (setup.sh generates for your domain)
├── caddy/
│   └── Caddyfile               # Bootstrap only; replaced by JSON API at startup
└── scripts/
    ├── setup.sh                # First-time server setup (one-command, idempotent)
    └── doctor.sh               # Diagnose and auto-fix (ports, DNS, Docker, Caddy)
```

## Configuration

All via environment variables:

```env
# Required
ROUTEROOT_DOMAIN=routeroot.dev         # Primary domain
ROUTEROOT_SERVER_IP=YOUR_SERVER_IP     # Server public IP (must match DNS zone A records)
ROUTEROOT_API_KEY=<random-secret>      # API auth (min 16 chars, rejects insecure defaults)

# Multi-domain (optional)
ROUTEROOT_DOMAINS=routeroot.dev,vibeyard.io  # Comma-separated list of all domains

# Optional
ROUTEROOT_MAX_DEPLOYMENTS=20          # Concurrent deployment limit
ROUTEROOT_DEFAULT_TTL=48h             # Auto-expire deployments
ROUTEROOT_MAX_MEMORY=2048             # MB per container
ROUTEROOT_MAX_CPUS=2                  # CPUs per container
ROUTEROOT_GITHUB_WEBHOOK_SECRET=...   # For auto-deploy on push
ROUTEROOT_LOG_FORMAT=human            # Set to 'json' for structured logging
ROUTEROOT_ALLOWED_REPO_HOSTS=github.com,gitlab.com,bitbucket.org  # Allowed git hosts

# Internal (set by docker-compose.yml)
ROUTEROOT_CADDY_ADMIN=http://caddy:2019  # Caddy JSON admin API
DATABASE_PATH=/data/routeroot.db          # SQLite DB path inside container
ZONE_FILE_DIR=/dns-zones                  # CoreDNS zone file directory (one per domain)
```

## Build Detection

The builder auto-detects how to build a repo:

| File Found | Action |
|---|---|
| `Dockerfile` | `docker build .` |
| `package.json` + framework detected | Node image + `npm install && npm run build && npm start` |
| `Cargo.toml` | Rust builder image + `cargo build --release` |
| `go.mod` | Go builder image + `go build` |
| `index.html` (static) | Caddy file server |

## Phases

### Phase 1: Foundation (MVP) -- COMPLETE
- [x] Agent API skeleton (Axum + SQLite + auth)
- [x] Docker service (bollard): run/stop/logs/list
- [x] Caddy integration: dynamic route registration via JSON API (replaces Caddyfile at startup)
- [x] CoreDNS config with wildcard zone
- [x] docker-compose.yml for full stack
- [x] Deploy endpoint: git clone → docker build → run → route → return URL
- [x] Teardown endpoint
- [x] List/status endpoints
- [x] TTL-based cleanup task
- [x] Plan/Apply pattern for safe agent-driven deployments
- [x] DNS record management (CRUD, with NS/SOA/CAA protection)
- [x] Custom domain mapping
- [x] Path-based routing (path-prefix routes inserted before root domain catch-all)
- [x] Deployment promotion (preview → staging → production)
- [x] Audit log on all mutations
- [x] Deployment verification (DNS + HTTP health checks)
- [x] On-demand TLS via Caddy JSON API config
- [x] Multi-domain support (per-domain zone files, Caddy TLS policies, routes)
- [x] Security hardening (constant-time auth, repo allowlist, zone injection prevention, CORS, container hardening, internal error masking, network isolation)

### Phase 2: CLI + MCP + Webhooks -- COMPLETE
- [x] CLI tool wrapping the API (deploy, ls, status, logs, down, plan, apply, promote, record, domain, audit, health, setup)
- [x] MCP server (15 tools, stdio transport)
- [x] GitHub webhook handler (push → deploy, delete → teardown)
- [x] Build detection (Dockerfile, Node, Rust, Go, Python, static)
- [x] Container log streaming
- [x] Setup script (one-command server setup, idempotent)
- [x] Doctor script (diagnose and auto-fix)
- [x] Systemd service + watchdog cron

**Architecture decisions made during implementation:**
- Agent-API runs in Docker, connects to host Docker via mounted socket
- `docker-ce-cli` installed in agent-api container for `docker build` commands
- Caddy configured via JSON API at startup (`init_caddy_config`), not just Caddyfile
- Deployed containers bind `0.0.0.0` so Caddy reaches them via `host.docker.internal`
- `extra_hosts: ["host.docker.internal:host-gateway"]` required on Caddy container

### Phase 3: Multi-Server / Geo-Distribution
- [ ] Node registry: API to add/remove servers (Hetzner, OVH, etc.)
- [ ] Server model in DB: id, name, ip, region, capacity, status
- [ ] GeoDNS via CoreDNS `geoip` plugin — route to nearest server
- [ ] Deploy target selection: auto (least-loaded) or manual (`--server eu-1`)
- [ ] Agent worker on each server: receives build instructions from control plane
- [ ] Server health monitoring + auto-failover
- [ ] Add server: `routeroot server add --name eu-hetzner-1 --ip x.x.x.x --region eu`
- [ ] DNS records dynamically point subdomains to the server hosting that deployment
- [ ] Wildcard per-server: `*.eu.routeroot.dev`, `*.us.routeroot.dev`

### Phase 4: Polish
- [ ] Health dashboard (simple HTML page at root domain)
- [ ] Resource usage tracking per deployment
- [ ] Deploy notifications (optional webhook/slack)
- [x] Custom domain mapping (point any domain at a deployment) — implemented
- [x] Deployment promotion: `routeroot promote staging → production` — implemented

## Multi-Server Architecture

```
                     ┌──────────────────────┐
                     │  Control Plane        │
                     │  (Agent API + CoreDNS)│
                     │  routeroot.dev NS      │
                     └──────────┬───────────┘
                                │ orchestrates
              ┌─────────────────┼─────────────────┐
              ▼                 ▼                  ▼
    ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
    │  eu-hetzner-1   │ │  eu-hetzner-2   │ │  us-hetzner-1   │
    │  Caddy + Docker │ │  Caddy + Docker │ │  Caddy + Docker │
    │  Agent Worker   │ │  Agent Worker   │ │  Agent Worker   │
    │  65.x.x.x       │ │  65.x.x.y       │ │  5.x.x.z        │
    └─────────────────┘ └─────────────────┘ └─────────────────┘
```

**How it works:**
1. Control plane receives deploy request
2. Selects target server (auto: least-loaded in nearest region, or manual)
3. Sends build+run instruction to agent worker on that server via authenticated API
4. Worker clones, builds, runs container, configures local Caddy
5. Control plane updates DNS: `myapp.routeroot.dev A → target server IP`
6. CoreDNS serves the record, traffic goes directly to the worker server

**Adding a new Hetzner box:**
```bash
# On the new server:
curl -sSL https://raw.githubusercontent.com/Vibeyard/RouteRoot/main/scripts/setup-worker.sh | bash

# From control plane:
routeroot server add --name eu-hetzner-3 --ip 65.x.x.z --region eu-central
```

Each worker runs Caddy + Docker + a thin agent worker service.
The control plane is the only one running CoreDNS (authoritative DNS).

## Server Requirements

- Linux server with Docker installed
- Ports open: 53 (DNS), 80 (HTTP), 443 (HTTPS)
- A domain with NS records pointing to this server
- ~2GB RAM minimum for the platform itself, plus resources for deployments

Note: Port 8053 (API) is NOT opened externally — it's localhost-only. External API access goes through Caddy at `https://api.yourdomain`.

## Security

- **API key required** — Min 16 chars, rejects known defaults (`dev-key`, `change-me`, etc.)
- **Constant-time auth** — HMAC-based key comparison prevents timing attacks
- **Repo URL allowlist** — Only HTTPS repos from configured hosts
- **DNS zone injection prevention** — Record names/types/values validated against metacharacters
- **Protected DNS records** — NS, SOA, CAA cannot be created/deleted via API
- **TLS cert scoping** — Only issues certs for subdomains with active deployments
- **API port localhost-only** — Port 8053 bound to `127.0.0.1`; external access via Caddy HTTPS
- **Internal Docker network** — Caddy admin API not exposed outside internal bridge
- **CORS restricted** — Only managed domain origins allowed
- **Container hardening** — `no-new-privileges`, PID limits, memory/CPU limits, empty binds, tmpfs /tmp
- **Internal error masking** — Real errors logged server-side, generic messages to clients
- **Audit log** — All mutations logged
- **Webhook signature verification** — GitHub HMAC-SHA256
