# AgentDNS

A self-hosted DNS + deploy platform for instant preview deployments, demos, and API routing.
Built for agentic development workflows where branches become live URLs in seconds.

## Problem

Deploying preview branches, demos, and staging environments requires manual DNS config,
vendor dashboards (Cloudflare, Railway, Vercel), and per-project setup. We want:

- `curl deploy.agentdns/deploy --data '{"repo":"...", "branch":"..."}'` → live URL in 60s
- Zero vendor dependencies for dev/demo infrastructure
- Agentic CI/CD: Claude Code or any agent can deploy and verify branches autonomously

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│  Domain: *.agentdns.dev (or similar)                     │
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
- Wildcard `*.agentdns.dev → server IP` handles 99% of cases
- File-based zones with 5s auto-reload for custom records
- Plugin: `file` for zone management, `log`, `errors`, `health`

### 2. Agent API (Rust + Axum)
The brain of the system. Single binary that orchestrates everything.

**Endpoints:**
```
POST   /api/deploy              Deploy a branch (clone → build → run → route)
DELETE /api/deploy/:name        Tear down a deployment
GET    /api/deployments         List active deployments
GET    /api/deployments/:name   Deployment details + status
GET    /api/deployments/:name/logs  Stream container logs

POST   /api/records             Create custom DNS record
GET    /api/records             List DNS records
DELETE /api/records/:name       Delete DNS record

GET    /api/health              System health (CoreDNS, Caddy, Docker, disk)
```

**Deploy flow:**
1. Receive deploy request (repo URL, branch, optional subdomain name)
2. Clone repo, detect build system (Dockerfile / package.json / Cargo.toml)
3. Build Docker image
4. Allocate port, start container with resource limits
5. Register route in Caddy via admin API
6. Return `https://{name}.agentdns.dev`
7. Background: health check loop, auto-expire after TTL

**Tech:**
- Rust + Axum for the HTTP server
- Bollard for Docker API (no shelling out)
- SQLite (via rusqlite) for state
- Tokio for async

### 3. Caddy (off-the-shelf)
- On-demand TLS: auto-provisions Let's Encrypt certs per subdomain
- Admin API at `:2019` for dynamic route registration
- Validation endpoint: API confirms subdomain is a real deployment before cert issuance

### 4. CLI (`agentdns`)
Thin Rust CLI that wraps the API.

```bash
agentdns deploy <repo> [--branch <branch>] [--name <name>] [--ttl 48h]
agentdns ls
agentdns logs <name> [--follow]
agentdns down <name>
agentdns status
agentdns record add <subdomain> <type> <value>
agentdns record ls
agentdns record rm <subdomain>
```

### 5. GitHub Webhook Handler (in Agent API)
- Listens for push events
- Auto-deploys branches matching configurable patterns
- Auto-tears down on branch delete
- Updates deployment on force-push

## Project Structure

```
AgentDNS/
├── PLAN.md
├── CLAUDE.md
├── docker-compose.yml          # Full stack: CoreDNS + Caddy + Agent API
├── agent-api/                  # Rust Axum service
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs
│   │   ├── config.rs           # Env-based config
│   │   ├── routes/
│   │   │   ├── mod.rs
│   │   │   ├── deploy.rs       # Deploy/teardown endpoints
│   │   │   ├── records.rs      # DNS record CRUD
│   │   │   ├── health.rs       # System health
│   │   │   └── webhook.rs      # GitHub webhook handler
│   │   ├── services/
│   │   │   ├── mod.rs
│   │   │   ├── docker.rs       # Container lifecycle (bollard)
│   │   │   ├── dns.rs          # Zone file writer + CoreDNS reload
│   │   │   ├── proxy.rs        # Caddy admin API client
│   │   │   ├── builder.rs      # Repo clone + image build
│   │   │   └── cleanup.rs      # TTL-based reaper task
│   │   ├── db.rs               # SQLite schema + queries
│   │   ├── error.rs            # Error types
│   │   └── auth.rs             # API key middleware
│   └── Dockerfile
├── cli/                        # Rust CLI
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
├── coredns/
│   ├── Corefile
│   └── zones/
│       └── db.agentdns.dev     # Zone template
├── caddy/
│   └── Caddyfile
└── scripts/
    ├── setup.sh                # First-time server setup
    └── install-cli.sh          # Install CLI locally
```

## Configuration

All via environment variables:

```env
# Required
AGENTDNS_DOMAIN=agentdns.dev         # Your domain
AGENTDNS_SERVER_IP=51.178.209.71     # Server public IP
AGENTDNS_API_KEY=<random-secret>     # API auth

# Optional
AGENTDNS_MAX_DEPLOYMENTS=20          # Concurrent deployment limit
AGENTDNS_DEFAULT_TTL=48h             # Auto-expire deployments
AGENTDNS_MAX_MEMORY=2048             # MB per container
AGENTDNS_MAX_CPUS=2                  # CPUs per container
AGENTDNS_GITHUB_WEBHOOK_SECRET=...   # For auto-deploy on push
AGENTDNS_CADDY_ADMIN=http://caddy:2019
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

### Phase 1: Foundation (MVP)
- [ ] Agent API skeleton (Axum + SQLite + auth)
- [ ] Docker service (bollard): run/stop/logs/list
- [ ] Caddy integration: dynamic route registration via admin API
- [ ] CoreDNS config with wildcard zone
- [ ] docker-compose.yml for full stack
- [ ] Deploy endpoint: git clone → docker build → run → route → return URL
- [ ] Teardown endpoint
- [ ] List/status endpoints
- [ ] TTL-based cleanup task

### Phase 2: CLI + Webhooks
- [ ] CLI tool wrapping the API
- [ ] GitHub webhook handler (push → deploy, delete → teardown)
- [ ] Build detection (Dockerfile, Node, Rust, Go, static)
- [ ] Container log streaming

### Phase 3: Multi-Server / Geo-Distribution
- [ ] Node registry: API to add/remove servers (Hetzner, OVH, etc.)
- [ ] Server model in DB: id, name, ip, region, capacity, status
- [ ] GeoDNS via CoreDNS `geoip` plugin — route to nearest server
- [ ] Deploy target selection: auto (least-loaded) or manual (`--server eu-1`)
- [ ] Agent worker on each server: receives build instructions from control plane
- [ ] Server health monitoring + auto-failover
- [ ] Add server: `agentdns server add --name eu-hetzner-1 --ip x.x.x.x --region eu`
- [ ] DNS records dynamically point subdomains to the server hosting that deployment
- [ ] Wildcard per-server: `*.eu.agentdns.dev`, `*.us.agentdns.dev`

### Phase 4: Polish
- [ ] Health dashboard (simple HTML page at root domain)
- [ ] Resource usage tracking per deployment
- [ ] Deploy notifications (optional webhook/slack)
- [ ] Custom domain mapping (point any domain at a deployment)
- [ ] Deployment promotion: `agentdns promote staging → production`

## Multi-Server Architecture

```
                     ┌──────────────────────┐
                     │  Control Plane        │
                     │  (Agent API + CoreDNS)│
                     │  agentdns.dev NS      │
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
5. Control plane updates DNS: `myapp.agentdns.dev A → target server IP`
6. CoreDNS serves the record, traffic goes directly to the worker server

**Adding a new Hetzner box:**
```bash
# On the new server:
curl -sSL https://raw.githubusercontent.com/Vibeyard/AgentDNS/main/scripts/setup-worker.sh | bash

# From control plane:
agentdns server add --name eu-hetzner-3 --ip 65.x.x.z --region eu-central
```

Each worker runs Caddy + Docker + a thin agent worker service.
The control plane is the only one running CoreDNS (authoritative DNS).

## Server Requirements

- Linux server with Docker installed
- Ports open: 53 (DNS), 80 (HTTP), 443 (HTTPS), 8053 (API)
- A domain with NS records pointing to this server
- ~2GB RAM minimum for the platform itself, plus resources for deployments

## Security (Dev-Grade)

- API key auth on all endpoints (Bearer token)
- Container resource limits (memory, CPU, no privileged mode)
- No host networking for deployed containers
- Rate limiting on deploy endpoint
- Webhook signature verification for GitHub
- Not designed for hostile multi-tenant use — this is team infrastructure
