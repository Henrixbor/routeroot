#!/usr/bin/env bash
set -euo pipefail

# AgentDNS — One-command setup
# Usage: sudo bash scripts/setup.sh routeroot.dev
# Or:    sudo bash scripts/setup.sh (will prompt)

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
    SERVER_IP=$(ip -4 addr show eth0 2>/dev/null | grep -oP 'inet \K[\d.]+' | head -1 || echo "")
fi

# --- Get domain from argument or prompt ---
DOMAIN="${1:-}"
if [ -z "$DOMAIN" ]; then
    echo -n "  Domain (e.g. routeroot.dev): " </dev/tty
    read -r DOMAIN </dev/tty
fi
if [ -z "$DOMAIN" ]; then
    echo "  Error: domain is required."
    echo "  Usage: sudo bash scripts/setup.sh yourdomain.dev"
    exit 1
fi

if [ -z "$SERVER_IP" ]; then
    echo -n "  Could not detect public IPv4. Enter it manually: " </dev/tty
    read -r SERVER_IP </dev/tty
fi

echo "  Server IP:  $SERVER_IP"
echo "  Domain:     $DOMAIN"
echo "  Install to: $INSTALL_DIR"
echo ""
echo -n "  Continue? [Y/n] " </dev/tty
read -r CONFIRM </dev/tty
if [[ "${CONFIRM:-Y}" =~ ^[Nn] ]]; then
    echo "  Aborted."
    exit 0
fi
echo ""

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
echo "[4/7] Copying files to $INSTALL_DIR..."
if [ -d "$INSTALL_DIR" ] && [ -d "$INSTALL_DIR/.git" ]; then
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
echo "[5/7] Generating config..."
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

echo "  .env written (API key generated)"
echo "  Zone file written for $DOMAIN"

# --- Step 6: Firewall ---
if command -v ufw &>/dev/null && ufw status 2>/dev/null | grep -q "active"; then
    echo "[6/7] Configuring firewall..."
    ufw allow 53/udp >/dev/null
    ufw allow 53/tcp >/dev/null
    ufw allow 80/tcp >/dev/null
    ufw allow 443/tcp >/dev/null
    ufw allow 8053/tcp >/dev/null
else
    echo "[6/7] Firewall (skipped — ufw not active)"
fi

# --- Step 7: Build and start ---
echo "[7/7] Building and starting services..."
echo "  (Rust compile takes 3-5 min on first run)"
echo ""

# Build and start directly — NOT through systemd for initial setup
docker compose up -d --build --remove-orphans 2>&1 | tail -20

# Now install systemd + watchdog for self-healing AFTER successful start
echo ""
echo "  Installing self-healing (systemd + watchdog)..."

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
        log "ERROR: Still unhealthy. Full restart..."
        docker compose down && docker compose up -d >> "$LOG" 2>&1
    fi
    log "Recovery complete."
fi

if [ -f "$LOG" ] && [ "$(stat -c%s "$LOG" 2>/dev/null || stat -f%z "$LOG" 2>/dev/null)" -gt 1048576 ]; then
    tail -100 "$LOG" > "$LOG.tmp" && mv "$LOG.tmp" "$LOG"
fi
WATCHDOG

chmod +x /usr/local/bin/agentdns-watchdog
CRON_LINE="*/2 * * * * /usr/local/bin/agentdns-watchdog"
(crontab -l 2>/dev/null | grep -v agentdns-watchdog; echo "$CRON_LINE") | crontab -

systemctl daemon-reload
systemctl enable agentdns.service 2>/dev/null

echo "  Systemd service enabled (auto-start on reboot)"
echo "  Watchdog cron installed (health check every 2 min)"

# --- Wait for health ---
echo ""
echo "  Waiting for API to come up..."

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
        echo "  Test:      curl http://$SERVER_IP:8053/api/health"
        echo "  Logs:      cd $INSTALL_DIR && docker compose logs -f"
        echo "  Status:    systemctl status agentdns"
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
echo ""
echo "  Still building. This is normal for the first run."
echo "  Watch progress:  cd $INSTALL_DIR && docker compose logs -f agent-api"
echo "  Test when ready: curl http://localhost:8053/api/health"
echo "  API Key: $API_KEY"
echo ""
