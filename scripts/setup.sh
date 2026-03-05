#!/usr/bin/env bash
set -euo pipefail

# AgentDNS — One-command setup
# Usage: git clone ... && cd routeroot && sudo bash scripts/setup.sh
# Or:    sudo bash scripts/setup.sh routeroot.dev

REPO_URL="https://github.com/Henrixbor/routeroot.git"
INSTALL_DIR="/opt/agentdns"

echo ""
echo "  ╔═══════════════════════════════════════╗"
echo "  ║         AgentDNS Setup                ║"
echo "  ║   Self-hosted deploy platform         ║"
echo "  ╚═══════════════════════════════════════╝"
echo ""

# --- Detect IPv4 address (force -4 to avoid IPv6) ---
SERVER_IP=$(curl -4 -sf --max-time 5 ifconfig.me 2>/dev/null || curl -4 -sf --max-time 5 icanhazip.com 2>/dev/null || echo "")
if [ -z "$SERVER_IP" ]; then
    # Fallback: grab IPv4 from ip command
    SERVER_IP=$(ip -4 addr show eth0 2>/dev/null | grep -oP 'inet \K[\d.]+' | head -1 || echo "")
fi

# --- Get domain from argument or prompt ---
DOMAIN="${1:-}"
if [ -z "$DOMAIN" ]; then
    echo -n "Domain (e.g. routeroot.dev): " </dev/tty
    read -r DOMAIN </dev/tty
fi
if [ -z "$DOMAIN" ]; then
    echo "Error: domain is required. Usage: sudo bash scripts/setup.sh yourdomain.dev"
    exit 1
fi

# --- Allow overriding IP ---
if [ -z "$SERVER_IP" ]; then
    echo -n "Could not detect public IPv4. Enter it manually: " </dev/tty
    read -r SERVER_IP </dev/tty
fi

echo ""
echo "  Server IP:  $SERVER_IP"
echo "  Domain:     $DOMAIN"
echo "  Install to: $INSTALL_DIR"
echo ""
echo -n "Continue? [Y/n] " </dev/tty
read -r CONFIRM </dev/tty
if [[ "${CONFIRM:-Y}" =~ ^[Nn] ]]; then
    echo "Aborted."
    exit 0
fi

# --- Install Docker if missing ---
if ! command -v docker &>/dev/null; then
    echo ""
    echo "[1/6] Installing Docker..."
    curl -fsSL https://get.docker.com | sh
    systemctl enable docker
    systemctl start docker
    usermod -aG docker "$USER" 2>/dev/null || true
else
    echo "[1/6] Docker already installed."
fi

# --- Install Docker Compose plugin if missing ---
if ! docker compose version &>/dev/null; then
    echo "[2/6] Installing Docker Compose plugin..."
    apt-get update -qq && apt-get install -y -qq docker-compose-plugin
else
    echo "[2/6] Docker Compose already installed."
fi

# --- Ensure dependencies ---
echo "[3/6] Ensuring dependencies..."
apt-get install -y -qq git curl openssl >/dev/null 2>&1 || true

# --- Clone or update repo ---
echo "[4/6] Setting up AgentDNS..."
if [ -d "$INSTALL_DIR/.git" ]; then
    cd "$INSTALL_DIR"
    git pull --ff-only 2>/dev/null || true
else
    if [ -f "docker-compose.yml" ] && [ -d "agent-api" ]; then
        if [ "$(pwd)" != "$INSTALL_DIR" ]; then
            mkdir -p "$INSTALL_DIR"
            cp -r . "$INSTALL_DIR/"
        fi
        cd "$INSTALL_DIR"
    else
        git clone "$REPO_URL" "$INSTALL_DIR"
        cd "$INSTALL_DIR"
    fi
fi

# --- Generate config ---
echo "[5/6] Generating configuration..."
mkdir -p data coredns/zones

API_KEY=$(openssl rand -hex 32)

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

# Generate initial zone file
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

; Wildcard — all subdomains resolve to this server
*       IN A    ${SERVER_IP}
EOF

# --- Firewall ---
if command -v ufw &>/dev/null && ufw status 2>/dev/null | grep -q "active"; then
    echo "    Configuring firewall..."
    ufw allow 53/udp >/dev/null
    ufw allow 53/tcp >/dev/null
    ufw allow 80/tcp >/dev/null
    ufw allow 443/tcp >/dev/null
    ufw allow 8053/tcp >/dev/null
fi

# --- Install systemd service + watchdog ---
echo "[6/6] Installing systemd service and watchdog..."

cat > /etc/systemd/system/agentdns.service <<EOF
[Unit]
Description=AgentDNS Deploy Platform
After=docker.service
Requires=docker.service

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=$INSTALL_DIR
ExecStart=/usr/bin/docker compose up -d --build --remove-orphans
ExecStop=/usr/bin/docker compose down
ExecReload=/usr/bin/docker compose up -d --build --remove-orphans
TimeoutStartSec=300

[Install]
WantedBy=multi-user.target
EOF

cat > /usr/local/bin/agentdns-watchdog <<'WATCHDOG'
#!/usr/bin/env bash
# AgentDNS self-healing watchdog — runs via cron every 2 minutes

INSTALL_DIR="/opt/agentdns"
LOG="/var/log/agentdns-watchdog.log"

log() { echo "$(date -Iseconds) $1" >> "$LOG"; }

cd "$INSTALL_DIR" || exit 1
RUNNING=$(docker compose ps --status running -q 2>/dev/null | wc -l)

if [ "$RUNNING" -lt 3 ]; then
    log "WARN: Only $RUNNING/3 services running. Restarting..."
    docker compose up -d --remove-orphans >> "$LOG" 2>&1
    log "Restart triggered."
    exit 0
fi

HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" --max-time 5 http://localhost:8053/api/health 2>/dev/null || echo "000")

if [ "$HTTP_CODE" != "200" ]; then
    log "WARN: API health check failed (HTTP $HTTP_CODE). Restarting agent-api..."
    docker compose restart agent-api >> "$LOG" 2>&1
    sleep 10
    HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" --max-time 5 http://localhost:8053/api/health 2>/dev/null || echo "000")
    if [ "$HTTP_CODE" != "200" ]; then
        log "ERROR: API still unhealthy after restart. Full restart..."
        docker compose down && docker compose up -d >> "$LOG" 2>&1
    fi
    log "Recovery complete."
fi

# Rotate log if > 1MB
if [ -f "$LOG" ] && [ "$(stat -c%s "$LOG" 2>/dev/null || stat -f%z "$LOG" 2>/dev/null)" -gt 1048576 ]; then
    tail -100 "$LOG" > "$LOG.tmp" && mv "$LOG.tmp" "$LOG"
fi
WATCHDOG

chmod +x /usr/local/bin/agentdns-watchdog

CRON_LINE="*/2 * * * * /usr/local/bin/agentdns-watchdog"
(crontab -l 2>/dev/null | grep -v agentdns-watchdog; echo "$CRON_LINE") | crontab -

systemctl daemon-reload
systemctl enable agentdns.service
systemctl start agentdns.service

# --- Wait for health ---
echo ""
echo "Starting AgentDNS... (building may take a few minutes on first run)"
echo ""

for i in $(seq 1 60); do
    if curl -sf http://localhost:8053/api/health >/dev/null 2>&1; then
        echo ""
        echo "  ╔═══════════════════════════════════════════════════╗"
        echo "  ║              AgentDNS is running!                 ║"
        echo "  ╚═══════════════════════════════════════════════════╝"
        echo ""
        echo "  Domain:    $DOMAIN"
        echo "  API:       http://$SERVER_IP:8053"
        echo "  API Key:   $API_KEY"
        echo ""
        echo "  Health:    curl http://$SERVER_IP:8053/api/health"
        echo "  Logs:      cd $INSTALL_DIR && docker compose logs -f"
        echo "  Status:    systemctl status agentdns"
        echo ""
        echo "  Self-healing: Enabled (systemd + watchdog cron every 2min)"
        echo ""
        echo "  DNS Setup (at your registrar):"
        echo "    Set nameservers for $DOMAIN to:"
        echo "      ns1.$DOMAIN -> $SERVER_IP"
        echo "      ns2.$DOMAIN -> $SERVER_IP"
        echo ""
        echo "  Save your API key somewhere safe!"
        echo ""
        exit 0
    fi
    printf "."
    sleep 5
done

echo ""
echo "  AgentDNS is still starting up (Rust build takes ~3-5 min on first run)."
echo "  Check progress with: cd $INSTALL_DIR && docker compose logs -f"
echo "  API Key: $API_KEY"
echo ""
