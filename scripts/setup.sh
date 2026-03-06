#!/usr/bin/env bash
set -euo pipefail

# AgentDNS — One-command setup
#
# Interactive:     sudo bash scripts/setup.sh
# Non-interactive: sudo bash scripts/setup.sh routeroot.dev
# With IP:         sudo bash scripts/setup.sh routeroot.dev 91.98.128.171
# Full:            sudo bash scripts/setup.sh routeroot.dev 91.98.128.171 my-api-key
#
# When domain is passed as argument, no prompts are shown.
# This makes it safe for Claude Code / SSH / CI usage.

REPO_URL="https://github.com/Henrixbor/routeroot.git"
INSTALL_DIR="/opt/agentdns"

echo ""
echo "  ╔═══════════════════════════════════════╗"
echo "  ║         AgentDNS Setup                ║"
echo "  ║   Self-hosted deploy platform         ║"
echo "  ╚═══════════════════════════════════════╝"
echo ""

# --- Parse arguments ---
DOMAIN="${1:-}"
SERVER_IP="${2:-}"
API_KEY="${3:-}"
INTERACTIVE=false

# --- Detect IPv4 (force -4 to avoid IPv6) ---
if [ -z "$SERVER_IP" ]; then
    SERVER_IP=$(curl -4 -sf --max-time 5 ifconfig.me 2>/dev/null \
             || curl -4 -sf --max-time 5 icanhazip.com 2>/dev/null \
             || ip -4 addr show eth0 2>/dev/null | grep -oP 'inet \K[\d.]+' | head -1 \
             || echo "")
fi

# --- Interactive mode if no domain argument ---
if [ -z "$DOMAIN" ]; then
    INTERACTIVE=true
    if [ -t 0 ] || [ -e /dev/tty ]; then
        echo -n "  Domain (e.g. routeroot.dev): " </dev/tty
        read -r DOMAIN </dev/tty
    fi
    if [ -z "$DOMAIN" ]; then
        echo "  Error: domain is required."
        echo "  Usage: sudo bash scripts/setup.sh <domain> [server-ip] [api-key]"
        exit 1
    fi
fi

if [ -z "$SERVER_IP" ]; then
    if [ "$INTERACTIVE" = true ] && { [ -t 0 ] || [ -e /dev/tty ]; }; then
        echo -n "  Could not detect public IPv4. Enter manually: " </dev/tty
        read -r SERVER_IP </dev/tty
    fi
    if [ -z "$SERVER_IP" ]; then
        echo "  Error: could not detect server IP."
        echo "  Usage: sudo bash scripts/setup.sh <domain> <server-ip>"
        exit 1
    fi
fi

# --- Generate API key if not provided ---
if [ -z "$API_KEY" ]; then
    API_KEY=$(openssl rand -hex 32)
fi

echo "  Domain:     $DOMAIN"
echo "  Server IP:  $SERVER_IP"
echo "  Install to: $INSTALL_DIR"
echo ""

# Confirm only in interactive mode
if [ "$INTERACTIVE" = true ] && { [ -t 0 ] || [ -e /dev/tty ]; }; then
    echo -n "  Continue? [Y/n] " </dev/tty
    read -r CONFIRM </dev/tty
    if [[ "${CONFIRM:-Y}" =~ ^[Nn] ]]; then
        echo "  Aborted."
        exit 0
    fi
    echo ""
fi

# --- Step 1: Docker ---
if ! command -v docker &>/dev/null; then
    echo "[1/7] Installing Docker..."
    curl -fsSL https://get.docker.com | sh
    systemctl enable docker
    systemctl start docker
    usermod -aG docker "$USER" 2>/dev/null || true
else
    echo "[1/7] Docker OK"
fi

# --- Step 2: Docker Compose ---
if ! docker compose version &>/dev/null; then
    echo "[2/7] Installing Docker Compose..."
    apt-get update -qq && apt-get install -y -qq docker-compose-plugin
else
    echo "[2/7] Docker Compose OK"
fi

# --- Step 3: Dependencies ---
echo "[3/7] Dependencies..."
apt-get install -y -qq git curl openssl >/dev/null 2>&1 || true

# --- Step 4: Repo ---
echo "[4/7] Setting up $INSTALL_DIR..."
if [ -d "$INSTALL_DIR/.git" ]; then
    cd "$INSTALL_DIR"
    git pull --ff-only 2>/dev/null || true
elif [ -f "docker-compose.yml" ] && [ -d "agent-api" ]; then
    if [ "$(pwd)" != "$INSTALL_DIR" ]; then
        rm -rf "$INSTALL_DIR"
        mkdir -p "$INSTALL_DIR"
        cp -r . "$INSTALL_DIR/"
    fi
    cd "$INSTALL_DIR"
else
    git clone "$REPO_URL" "$INSTALL_DIR"
    cd "$INSTALL_DIR"
fi

# --- Step 5: Config ---
echo "[5/7] Writing config..."
mkdir -p data coredns/zones

cat > .env <<EOF
AGENTDNS_DOMAIN=$DOMAIN
AGENTDNS_SERVER_IP=$SERVER_IP
AGENTDNS_API_KEY=$API_KEY
AGENTDNS_MAX_DEPLOYMENTS=20
AGENTDNS_DEFAULT_TTL=48h
AGENTDNS_MAX_MEMORY=2048
AGENTDNS_MAX_CPUS=2
AGENTDNS_LOG_FORMAT=json
EOF

# Generate Corefile with actual domain (CoreDNS doesn't support env var defaults)
cat > coredns/Corefile <<EOF
.:53 {
    file /etc/coredns/zones/db.${DOMAIN} ${DOMAIN}
    reload 5s
    log
    errors
    health :8054
    ready :8055
}
EOF

cat > "coredns/zones/db.$DOMAIN" <<EOF
\$ORIGIN ${DOMAIN}.
\$TTL 300

@       IN SOA  ns1.${DOMAIN}. admin.${DOMAIN}. (
                $(date +%Y%m%d%H)  ; serial
                3600        ; refresh
                900         ; retry
                604800      ; expire
                300         ; minimum TTL
)

@       IN NS   ns1.${DOMAIN}.
@       IN NS   ns2.${DOMAIN}.

ns1     IN A    ${SERVER_IP}
ns2     IN A    ${SERVER_IP}

@       IN A    ${SERVER_IP}

*       IN A    ${SERVER_IP}
EOF

echo "  .env and zone file written"

# --- Step 6: Firewall ---
if command -v ufw &>/dev/null && ufw status 2>/dev/null | grep -q "active"; then
    echo "[6/7] Configuring firewall..."
    ufw allow 53/udp >/dev/null
    ufw allow 53/tcp >/dev/null
    ufw allow 80/tcp >/dev/null
    ufw allow 443/tcp >/dev/null
    ufw allow 8053/tcp >/dev/null
else
    echo "[6/7] Firewall (skipped)"
fi

# --- Free required ports (53, 80, 443) ---
free_port() {
    local PORT=$1
    local PIDS
    PIDS=$(ss -tlnp 2>/dev/null | grep ":${PORT} " | grep -oP 'pid=\K[0-9]+' | sort -u)
    if [ -z "$PIDS" ]; then return; fi

    for PID in $PIDS; do
        local PROC
        PROC=$(ps -p "$PID" -o comm= 2>/dev/null || echo "unknown")
        echo "  Port $PORT in use by $PROC (pid $PID)"

        case "$PROC" in
            systemd-resolve*)
                echo "  -> Disabling systemd-resolved..."
                systemctl stop systemd-resolved 2>/dev/null || true
                systemctl disable systemd-resolved 2>/dev/null || true
                echo "nameserver 8.8.8.8" > /etc/resolv.conf
                echo "nameserver 1.1.1.1" >> /etc/resolv.conf
                ;;
            nginx*)
                echo "  -> Stopping nginx..."
                systemctl stop nginx 2>/dev/null || true
                systemctl disable nginx 2>/dev/null || true
                ;;
            apache*|httpd*)
                echo "  -> Stopping apache..."
                systemctl stop apache2 2>/dev/null || systemctl stop httpd 2>/dev/null || true
                systemctl disable apache2 2>/dev/null || systemctl disable httpd 2>/dev/null || true
                ;;
            caddy*)
                echo "  -> Stopping system caddy (we use our own in Docker)..."
                systemctl stop caddy 2>/dev/null || true
                systemctl disable caddy 2>/dev/null || true
                ;;
            *)
                echo "  -> Killing process $PID ($PROC)..."
                kill "$PID" 2>/dev/null || true
                sleep 1
                ;;
        esac
    done
}

echo "  Checking required ports..."
free_port 53
free_port 80
free_port 443

# --- Step 7: Build and start ---
echo "[7/7] Building and starting..."
echo "  Rust compile takes 3-5 min on first run."
echo ""

docker compose up -d --build --remove-orphans

# --- Self-healing: systemd + watchdog ---
echo ""
echo "  Installing self-healing..."

cat > /etc/systemd/system/agentdns.service <<EOF
[Unit]
Description=AgentDNS Deploy Platform
After=docker.service
Requires=docker.service

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=$INSTALL_DIR
ExecStart=/usr/bin/docker compose up -d --remove-orphans
ExecStop=/usr/bin/docker compose down
ExecReload=/usr/bin/docker compose up -d --build --remove-orphans
TimeoutStartSec=300

[Install]
WantedBy=multi-user.target
EOF

cat > /usr/local/bin/agentdns-watchdog <<'WATCHDOG'
#!/usr/bin/env bash
INSTALL_DIR="/opt/agentdns"
LOG="/var/log/agentdns-watchdog.log"
log() { echo "$(date -Iseconds) $1" >> "$LOG"; }

cd "$INSTALL_DIR" || exit 1
RUNNING=$(docker compose ps --status running -q 2>/dev/null | wc -l)
if [ "$RUNNING" -lt 3 ]; then
    log "WARN: Only $RUNNING/3 services running. Restarting..."
    docker compose up -d --remove-orphans >> "$LOG" 2>&1
    exit 0
fi

HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" --max-time 5 http://localhost:8053/api/health 2>/dev/null || echo "000")
if [ "$HTTP_CODE" != "200" ]; then
    log "WARN: Health check failed ($HTTP_CODE). Restarting agent-api..."
    docker compose restart agent-api >> "$LOG" 2>&1
    sleep 10
    HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" --max-time 5 http://localhost:8053/api/health 2>/dev/null || echo "000")
    if [ "$HTTP_CODE" != "200" ]; then
        log "ERROR: Still down. Full restart..."
        docker compose down && docker compose up -d >> "$LOG" 2>&1
    fi
fi

if [ -f "$LOG" ] && [ "$(stat -c%s "$LOG" 2>/dev/null || stat -f%z "$LOG" 2>/dev/null)" -gt 1048576 ]; then
    tail -100 "$LOG" > "$LOG.tmp" && mv "$LOG.tmp" "$LOG"
fi
WATCHDOG

chmod +x /usr/local/bin/agentdns-watchdog
(crontab -l 2>/dev/null | grep -v agentdns-watchdog; echo "*/2 * * * * /usr/local/bin/agentdns-watchdog") | crontab -

systemctl daemon-reload
systemctl enable agentdns.service 2>/dev/null

# --- Wait for health ---
echo ""
echo "  Waiting for API..."
for i in $(seq 1 60); do
    if curl -sf http://localhost:8053/api/health >/dev/null 2>&1; then
        HEALTH=$(curl -sf http://localhost:8053/api/health)
        echo ""
        echo "  ╔═══════════════════════════════════════════════════╗"
        echo "  ║              AgentDNS is running!                 ║"
        echo "  ╚═══════════════════════════════════════════════════╝"
        echo ""
        echo "  Domain:     $DOMAIN"
        echo "  API:        http://$SERVER_IP:8053"
        echo "  API Key:    $API_KEY"
        echo "  Health:     $HEALTH"
        echo ""
        echo "  Logs:       cd $INSTALL_DIR && docker compose logs -f"
        echo "  Update:     cd $INSTALL_DIR && git pull && docker compose up -d --build"
        echo ""
        echo "  DNS Setup (at your registrar):"
        echo "    ns1.$DOMAIN -> $SERVER_IP"
        echo "    ns2.$DOMAIN -> $SERVER_IP"
        echo ""
        echo "  SAVE YOUR API KEY!"
        echo ""
        exit 0
    fi
    printf "."
    sleep 5
done

echo ""
echo ""
echo "  Still building (normal for first run)."
echo "  Watch:  cd $INSTALL_DIR && docker compose logs -f agent-api"
echo "  Test:   curl http://localhost:8053/api/health"
echo ""
echo "  API Key: $API_KEY"
echo ""
