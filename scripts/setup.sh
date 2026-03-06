#!/usr/bin/env bash
set -euo pipefail

# RouteRoot — Rock-solid one-command setup
#
# Interactive:     sudo bash scripts/setup.sh
# Non-interactive: sudo bash scripts/setup.sh routeroot.dev
# With IP:         sudo bash scripts/setup.sh routeroot.dev 91.98.128.171
# Full:            sudo bash scripts/setup.sh routeroot.dev 91.98.128.171 my-api-key
#
# Idempotent — safe to re-run. Preserves existing API key on re-run.

REPO_URL="https://github.com/Henrixbor/routeroot.git"
INSTALL_DIR="/opt/routeroot"
LOG="/var/log/routeroot-setup.log"
REQUIRED_DISK_MB=5000

# --- Logging ---
log() { echo "$(date -Iseconds) $1" | tee -a "$LOG"; }
fail() { echo ""; echo "  FAIL: $1"; echo "$(date -Iseconds) FAIL: $1" >> "$LOG"; exit 1; }

echo ""
echo "  ╔═══════════════════════════════════════╗"
echo "  ║         RouteRoot Setup                ║"
echo "  ║   Self-hosted deploy platform         ║"
echo "  ╚═══════════════════════════════════════╝"
echo ""

# --- Must be root ---
if [ "$(id -u)" -ne 0 ]; then
    fail "Must run as root. Use: sudo bash scripts/setup.sh"
fi

# --- Parse arguments ---
DOMAIN="${1:-}"
SERVER_IP="${2:-}"
API_KEY="${3:-}"
INTERACTIVE=false

# --- Detect IPv4 (force -4, multiple fallbacks) ---
if [ -z "$SERVER_IP" ]; then
    SERVER_IP=$(curl -4 -sf --max-time 5 ifconfig.me 2>/dev/null || true)
    [ -z "$SERVER_IP" ] && SERVER_IP=$(curl -4 -sf --max-time 5 icanhazip.com 2>/dev/null || true)
    [ -z "$SERVER_IP" ] && SERVER_IP=$(curl -4 -sf --max-time 5 api.ipify.org 2>/dev/null || true)
    [ -z "$SERVER_IP" ] && SERVER_IP=$(ip -4 addr show eth0 2>/dev/null | grep -oP 'inet \K[\d.]+' | head -1 || true)
    [ -z "$SERVER_IP" ] && SERVER_IP=$(ip -4 addr show 2>/dev/null | grep 'scope global' | grep -oP 'inet \K[\d.]+' | head -1 || true)
fi

# Validate it's actually IPv4
if [ -n "$SERVER_IP" ] && echo "$SERVER_IP" | grep -q ":"; then
    # Got IPv6, try harder for IPv4
    SERVER_IP=$(ip -4 addr show 2>/dev/null | grep 'scope global' | grep -oP 'inet \K[\d.]+' | head -1 || echo "")
fi

# --- Interactive mode if no domain argument ---
if [ -z "$DOMAIN" ]; then
    INTERACTIVE=true
    if [ -t 0 ] || [ -e /dev/tty ]; then
        echo -n "  Domain (e.g. routeroot.dev): " </dev/tty
        read -r DOMAIN </dev/tty
    fi
    [ -z "$DOMAIN" ] && fail "Domain is required. Usage: sudo bash scripts/setup.sh <domain> [server-ip] [api-key]"
fi

# Validate domain format
if ! echo "$DOMAIN" | grep -qP '^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$'; then
    fail "Invalid domain format: $DOMAIN"
fi

if [ -z "$SERVER_IP" ]; then
    if [ "$INTERACTIVE" = true ] && { [ -t 0 ] || [ -e /dev/tty ]; }; then
        echo -n "  Could not detect public IPv4. Enter manually: " </dev/tty
        read -r SERVER_IP </dev/tty
    fi
    [ -z "$SERVER_IP" ] && fail "Could not detect server IP. Usage: sudo bash scripts/setup.sh <domain> <server-ip>"
fi

# --- Preserve existing API key on re-run ---
if [ -z "$API_KEY" ] && [ -f "$INSTALL_DIR/.env" ]; then
    API_KEY=$(grep '^ROUTEROOT_API_KEY=' "$INSTALL_DIR/.env" 2>/dev/null | cut -d= -f2 || true)
    if [ -n "$API_KEY" ]; then
        echo "  (Preserving existing API key)"
    fi
fi
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

# ============================================================
# PREFLIGHT CHECKS
# ============================================================

echo "  Preflight checks..."

# --- Check disk space ---
AVAIL_MB=$(df / --output=avail 2>/dev/null | tail -1 | tr -d ' ' || echo "999999")
AVAIL_MB=$((AVAIL_MB / 1024))
if [ "$AVAIL_MB" -lt "$REQUIRED_DISK_MB" ]; then
    echo "  WARN: Only ${AVAIL_MB}MB free (need ~${REQUIRED_DISK_MB}MB for Rust build)"
    echo "  Trying to free space..."
    apt-get clean 2>/dev/null || true
    docker system prune -f 2>/dev/null || true
    rm -rf /tmp/routeroot-builds 2>/dev/null || true
    AVAIL_MB=$(df / --output=avail 2>/dev/null | tail -1 | tr -d ' ' || echo "999999")
    AVAIL_MB=$((AVAIL_MB / 1024))
    if [ "$AVAIL_MB" -lt "$REQUIRED_DISK_MB" ]; then
        fail "Not enough disk space (${AVAIL_MB}MB free, need ${REQUIRED_DISK_MB}MB). Free space and retry."
    fi
    echo "  Freed space. Now ${AVAIL_MB}MB available."
fi
echo "  Disk: ${AVAIL_MB}MB free"

# --- Free required ports (53, 80, 443) ---
free_port() {
    local PORT=$1
    # Check both TCP and UDP for port 53
    local PIDS
    PIDS=$(ss -tlnp 2>/dev/null | grep ":${PORT} " | grep -oP 'pid=\K[0-9]+' | sort -u || true)
    [ "$PORT" = "53" ] && PIDS="$PIDS $(ss -ulnp 2>/dev/null | grep ":${PORT} " | grep -oP 'pid=\K[0-9]+' | sort -u || true)"
    PIDS=$(echo "$PIDS" | tr ' ' '\n' | sort -u | grep -v '^$' || true)

    if [ -z "$PIDS" ]; then
        echo "  Port $PORT: free"
        return
    fi

    for PID in $PIDS; do
        local PROC
        PROC=$(ps -p "$PID" -o comm= 2>/dev/null || echo "unknown")

        # Skip if it's our own Docker containers
        if grep -q docker "/proc/$PID/cgroup" 2>/dev/null; then
            continue
        fi

        echo "  Port $PORT: freeing ($PROC, pid $PID)"

        case "$PROC" in
            systemd-resolve*)
                systemctl stop systemd-resolved 2>/dev/null || true
                systemctl disable systemd-resolved 2>/dev/null || true
                # Backup and replace resolv.conf
                [ ! -f /etc/resolv.conf.pre-routeroot ] && cp /etc/resolv.conf /etc/resolv.conf.pre-routeroot 2>/dev/null || true
                rm -f /etc/resolv.conf  # may be a symlink
                printf "nameserver 8.8.8.8\nnameserver 1.1.1.1\n" > /etc/resolv.conf
                ;;
            nginx*)
                systemctl stop nginx 2>/dev/null || true
                systemctl disable nginx 2>/dev/null || true
                ;;
            apache*|httpd*)
                systemctl stop apache2 2>/dev/null || systemctl stop httpd 2>/dev/null || true
                systemctl disable apache2 2>/dev/null || systemctl disable httpd 2>/dev/null || true
                ;;
            caddy*)
                systemctl stop caddy 2>/dev/null || true
                systemctl disable caddy 2>/dev/null || true
                ;;
            dnsmasq*)
                systemctl stop dnsmasq 2>/dev/null || true
                systemctl disable dnsmasq 2>/dev/null || true
                ;;
            named*|bind*)
                systemctl stop named 2>/dev/null || systemctl stop bind9 2>/dev/null || true
                systemctl disable named 2>/dev/null || systemctl disable bind9 2>/dev/null || true
                ;;
            *)
                kill "$PID" 2>/dev/null || true
                sleep 1
                # Verify it's dead
                if kill -0 "$PID" 2>/dev/null; then
                    kill -9 "$PID" 2>/dev/null || true
                fi
                ;;
        esac
    done

    # Verify port is free now
    sleep 1
    if ss -tlnp 2>/dev/null | grep -q ":${PORT} "; then
        echo "  WARN: Port $PORT may still be in use"
    fi
}

free_port 53
free_port 80
free_port 443

# ============================================================
# INSTALLATION
# ============================================================

# --- Step 1: Docker ---
if ! command -v docker &>/dev/null; then
    echo "[1/7] Installing Docker..."
    curl -fsSL https://get.docker.com | sh
    systemctl enable docker
    systemctl start docker
    usermod -aG docker "$USER" 2>/dev/null || true
else
    echo "[1/7] Docker OK ($(docker --version | grep -oP '\d+\.\d+\.\d+'))"
fi

# Make sure Docker daemon is actually running
if ! docker info &>/dev/null; then
    echo "  Starting Docker daemon..."
    systemctl start docker
    sleep 3
    docker info &>/dev/null || fail "Docker daemon won't start. Check: journalctl -u docker"
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
ROUTEROOT_DOMAIN=$DOMAIN
ROUTEROOT_SERVER_IP=$SERVER_IP
ROUTEROOT_API_KEY=$API_KEY
ROUTEROOT_MAX_DEPLOYMENTS=20
ROUTEROOT_DEFAULT_TTL=48h
ROUTEROOT_MAX_MEMORY=2048
ROUTEROOT_MAX_CPUS=2
ROUTEROOT_LOG_FORMAT=json
EOF

# Corefile — must have actual domain, CoreDNS doesn't do env var substitution
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

# Save API key to file for easy retrieval
echo "$API_KEY" > "$INSTALL_DIR/.api-key"
chmod 600 "$INSTALL_DIR/.api-key"

echo "  Config written"

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

# --- Step 7: Build and start ---
echo "[7/7] Building and starting..."
echo "  Rust compile takes 3-5 min on first run."
echo ""

# Stop any existing containers first
docker compose down 2>/dev/null || true

# Build with output visible
if ! docker compose build 2>&1 | tee -a "$LOG"; then
    fail "Docker build failed. Check $LOG for details."
fi

# Start services
if ! docker compose up -d 2>&1 | tee -a "$LOG"; then
    echo ""
    echo "  Start failed. Running diagnostics..."
    echo ""
    # Check each service individually
    for SVC in coredns caddy agent-api; do
        if docker compose up -d "$SVC" 2>&1; then
            echo "  $SVC: OK"
        else
            echo "  $SVC: FAILED"
            docker compose logs "$SVC" --tail 5
        fi
    done
    fail "Could not start all services. Check logs above."
fi

# ============================================================
# SELF-HEALING
# ============================================================

echo ""
echo "  Installing self-healing..."

cat > /etc/systemd/system/routeroot.service <<EOF
[Unit]
Description=RouteRoot Deploy Platform
After=docker.service
Requires=docker.service

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=$INSTALL_DIR
ExecStartPre=/bin/bash -c 'ss -tlnp | grep -q "systemd-resolve.*:53" && systemctl stop systemd-resolved || true'
ExecStart=/usr/bin/docker compose up -d --remove-orphans
ExecStop=/usr/bin/docker compose down
ExecReload=/usr/bin/docker compose up -d --build --remove-orphans
TimeoutStartSec=300

[Install]
WantedBy=multi-user.target
EOF

cat > /usr/local/bin/routeroot-watchdog <<'WATCHDOG'
#!/usr/bin/env bash
INSTALL_DIR="/opt/routeroot"
LOG="/var/log/routeroot-watchdog.log"
log() { echo "$(date -Iseconds) $1" >> "$LOG"; }

cd "$INSTALL_DIR" || exit 1

# Fix systemd-resolved if it snuck back
if ss -tlnp 2>/dev/null | grep -q "systemd-resolve.*:53"; then
    log "FIXING: systemd-resolved reappeared on port 53"
    systemctl stop systemd-resolved 2>/dev/null || true
    systemctl disable systemd-resolved 2>/dev/null || true
    rm -f /etc/resolv.conf
    printf "nameserver 8.8.8.8\nnameserver 1.1.1.1\n" > /etc/resolv.conf
fi

# Check containers
RUNNING=$(docker compose ps --status running -q 2>/dev/null | wc -l)
if [ "$RUNNING" -lt 3 ]; then
    log "WARN: Only $RUNNING/3 services running. Restarting..."
    docker compose up -d --remove-orphans >> "$LOG" 2>&1
    sleep 15
    RUNNING=$(docker compose ps --status running -q 2>/dev/null | wc -l)
    if [ "$RUNNING" -lt 3 ]; then
        log "ERROR: Still only $RUNNING/3 after restart. Trying down+up..."
        docker compose down >> "$LOG" 2>&1
        docker compose up -d >> "$LOG" 2>&1
    fi
    exit 0
fi

# Check API health
HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" --max-time 5 http://localhost:8053/api/health 2>/dev/null || echo "000")
if [ "$HTTP_CODE" != "200" ]; then
    log "WARN: Health check failed ($HTTP_CODE). Restarting agent-api..."
    docker compose restart agent-api >> "$LOG" 2>&1
    sleep 10
    HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" --max-time 5 http://localhost:8053/api/health 2>/dev/null || echo "000")
    if [ "$HTTP_CODE" != "200" ]; then
        log "ERROR: Still unhealthy. Full restart..."
        docker compose down >> "$LOG" 2>&1
        docker compose up -d >> "$LOG" 2>&1
    fi
fi

# Rotate log
if [ -f "$LOG" ] && [ "$(stat -c%s "$LOG" 2>/dev/null || echo 0)" -gt 1048576 ]; then
    tail -200 "$LOG" > "$LOG.tmp" && mv "$LOG.tmp" "$LOG"
fi
WATCHDOG

chmod +x /usr/local/bin/routeroot-watchdog
(crontab -l 2>/dev/null | grep -v routeroot-watchdog; echo "*/2 * * * * /usr/local/bin/routeroot-watchdog") | crontab -

systemctl daemon-reload
systemctl enable routeroot.service 2>/dev/null
echo "  Systemd + watchdog installed"

# ============================================================
# HEALTH CHECK
# ============================================================

echo ""
echo "  Waiting for API..."
for i in $(seq 1 30); do
    HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" --max-time 3 http://localhost:8053/api/health 2>/dev/null || echo "000")
    if [ "$HTTP_CODE" = "200" ]; then
        HEALTH=$(curl -sf http://localhost:8053/api/health 2>/dev/null || echo "{}")
        echo ""
        echo "  ╔═══════════════════════════════════════════════════╗"
        echo "  ║              RouteRoot is running!                 ║"
        echo "  ╚═══════════════════════════════════════════════════╝"
        echo ""
        echo "  Domain:     $DOMAIN"
        echo "  API:        http://$SERVER_IP:8053"
        echo "  API Key:    $API_KEY"
        echo "  Key file:   $INSTALL_DIR/.api-key"
        echo ""
        echo "  Test:       curl http://$SERVER_IP:8053/api/health"
        echo "  Logs:       cd $INSTALL_DIR && docker compose logs -f"
        echo "  Doctor:     cd $INSTALL_DIR && sudo bash scripts/doctor.sh"
        echo "  Update:     cd $INSTALL_DIR && sudo bash scripts/update.sh"
        echo ""
        echo "  DNS Setup (Namecheap):"
        echo "    1. Go to Domain List -> routeroot.dev -> Manage"
        echo "    2. Under Nameservers, select 'Custom DNS'"
        echo "    3. Add: ns1.$DOMAIN and ns2.$DOMAIN"
        echo "    4. Go to Advanced DNS -> Personal DNS Server"
        echo "    5. Add ns1.$DOMAIN -> $SERVER_IP"
        echo "    6. Add ns2.$DOMAIN -> $SERVER_IP"
        echo ""
        echo "  Setup log:  $LOG"
        echo ""
        log "Setup complete. Domain=$DOMAIN IP=$SERVER_IP"
        exit 0
    fi
    printf "."
    sleep 3
done

# If we get here, API didn't come up — diagnose
echo ""
echo ""
echo "  API didn't respond in 90s. Diagnosing..."
echo ""
echo "  Container status:"
docker compose ps -a
echo ""
echo "  Last 10 log lines:"
docker compose logs --tail 10
echo ""
echo "  Your API key: $API_KEY"
echo "  Key file:     $INSTALL_DIR/.api-key"
echo "  Try:          cd $INSTALL_DIR && sudo bash scripts/doctor.sh"
echo ""
