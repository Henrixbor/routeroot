#!/usr/bin/env bash
set -euo pipefail

# RouteRoot Doctor — diagnose and auto-fix everything
# Usage: sudo bash scripts/doctor.sh

INSTALL_DIR="${ROUTEROOT_DIR:-/opt/routeroot}"
FIXES=0
ERRORS=0

echo ""
echo "  RouteRoot Doctor"
echo "  ==============="
echo ""

cd "$INSTALL_DIR" 2>/dev/null || { echo "  FAIL: $INSTALL_DIR not found. Run setup.sh first."; exit 1; }

# --- Check root ---
if [ "$(id -u)" -ne 0 ]; then
    echo "  FAIL: Must run as root (sudo bash scripts/doctor.sh)"
    exit 1
fi

# --- Check .env ---
if [ ! -f .env ]; then
    echo "  FAIL: .env missing. Re-run setup.sh"
    exit 1
fi
source .env 2>/dev/null || { echo "  FAIL: .env is malformed"; exit 1; }
echo "  OK: .env (domain=$ROUTEROOT_DOMAIN, ip=$ROUTEROOT_SERVER_IP)"

# --- Check .env has IPv4 not IPv6 ---
if echo "$ROUTEROOT_SERVER_IP" | grep -q ":"; then
    REAL_IP=$(curl -4 -sf --max-time 5 ifconfig.me 2>/dev/null \
           || curl -4 -sf --max-time 5 icanhazip.com 2>/dev/null \
           || ip -4 addr show 2>/dev/null | grep 'scope global' | grep -oP 'inet \K[\d.]+' | head -1 \
           || echo "")
    if [ -n "$REAL_IP" ]; then
        echo "  FIXING: .env has IPv6 ($ROUTEROOT_SERVER_IP) -> IPv4 ($REAL_IP)"
        sed -i "s/ROUTEROOT_SERVER_IP=.*/ROUTEROOT_SERVER_IP=$REAL_IP/" .env
        ROUTEROOT_SERVER_IP="$REAL_IP"
        FIXES=$((FIXES + 1))
    else
        echo "  ERROR: .env has IPv6 but can't detect IPv4"
        ERRORS=$((ERRORS + 1))
    fi
else
    echo "  OK: Server IP is IPv4"
fi

# --- Check port conflicts (53, 80, 443) ---
fix_port() {
    local PORT=$1
    local PIDS
    PIDS=$(ss -tlnp 2>/dev/null | grep ":${PORT} " | grep -oP 'pid=\K[0-9]+' | sort -u || true)
    [ "$PORT" = "53" ] && PIDS="$PIDS $(ss -ulnp 2>/dev/null | grep ":${PORT} " | grep -oP 'pid=\K[0-9]+' | sort -u || true)"
    PIDS=$(echo "$PIDS" | tr ' ' '\n' | sort -u | grep -v '^$' || true)

    # Filter out Docker-managed processes
    local NON_DOCKER_PIDS=""
    for PID in $PIDS; do
        if ! grep -q docker "/proc/$PID/cgroup" 2>/dev/null; then
            NON_DOCKER_PIDS="$NON_DOCKER_PIDS $PID"
        fi
    done
    NON_DOCKER_PIDS=$(echo "$NON_DOCKER_PIDS" | tr ' ' '\n' | grep -v '^$' | sort -u || true)

    if [ -z "$NON_DOCKER_PIDS" ]; then
        echo "  OK: Port $PORT"
        return
    fi

    for PID in $NON_DOCKER_PIDS; do
        local PROC
        PROC=$(ps -p "$PID" -o comm= 2>/dev/null || echo "unknown")
        echo "  FIXING: Port $PORT used by $PROC (pid $PID)"

        case "$PROC" in
            systemd-resolve*)
                systemctl stop systemd-resolved 2>/dev/null || true
                systemctl disable systemd-resolved 2>/dev/null || true
                rm -f /etc/resolv.conf
                printf "nameserver 8.8.8.8\nnameserver 1.1.1.1\n" > /etc/resolv.conf
                echo "  FIXED: systemd-resolved disabled"
                ;;
            nginx*)
                systemctl stop nginx 2>/dev/null || true
                systemctl disable nginx 2>/dev/null || true
                echo "  FIXED: nginx stopped"
                ;;
            apache*|httpd*)
                systemctl stop apache2 2>/dev/null || systemctl stop httpd 2>/dev/null || true
                systemctl disable apache2 2>/dev/null || systemctl disable httpd 2>/dev/null || true
                echo "  FIXED: apache stopped"
                ;;
            caddy*)
                systemctl stop caddy 2>/dev/null || true
                systemctl disable caddy 2>/dev/null || true
                echo "  FIXED: system caddy stopped"
                ;;
            dnsmasq*)
                systemctl stop dnsmasq 2>/dev/null || true
                systemctl disable dnsmasq 2>/dev/null || true
                echo "  FIXED: dnsmasq stopped"
                ;;
            named*|bind*)
                systemctl stop named 2>/dev/null || systemctl stop bind9 2>/dev/null || true
                systemctl disable named 2>/dev/null || systemctl disable bind9 2>/dev/null || true
                echo "  FIXED: bind stopped"
                ;;
            *)
                kill "$PID" 2>/dev/null || true
                sleep 1
                kill -0 "$PID" 2>/dev/null && kill -9 "$PID" 2>/dev/null
                echo "  FIXED: killed $PROC ($PID)"
                ;;
        esac
        FIXES=$((FIXES + 1))
    done
}

fix_port 53
fix_port 80
fix_port 443

# --- Check Docker ---
if ! command -v docker &>/dev/null; then
    echo "  FAIL: Docker not installed"
    exit 1
fi
if ! docker info &>/dev/null; then
    echo "  FIXING: Docker daemon not running"
    systemctl start docker
    sleep 3
    docker info &>/dev/null || { echo "  FAIL: Docker won't start"; exit 1; }
    echo "  FIXED: Docker started"
    FIXES=$((FIXES + 1))
else
    echo "  OK: Docker running"
fi

# --- Check Corefile ---
if grep -q '{\$ROUTEROOT_DOMAIN' coredns/Corefile 2>/dev/null || ! grep -q "$ROUTEROOT_DOMAIN" coredns/Corefile 2>/dev/null; then
    echo "  FIXING: Corefile (domain mismatch or env var placeholder)"
    cat > coredns/Corefile <<EOF
.:53 {
    file /etc/coredns/zones/db.${ROUTEROOT_DOMAIN} ${ROUTEROOT_DOMAIN}
    reload 5s
    log
    errors
    health :8054
    ready :8055
}
EOF
    echo "  FIXED: Corefile for $ROUTEROOT_DOMAIN"
    FIXES=$((FIXES + 1))
else
    echo "  OK: Corefile"
fi

# --- Check zone file ---
if [ ! -f "coredns/zones/db.${ROUTEROOT_DOMAIN}" ]; then
    echo "  FIXING: Zone file missing"
    mkdir -p coredns/zones
    cat > "coredns/zones/db.${ROUTEROOT_DOMAIN}" <<EOF
\$ORIGIN ${ROUTEROOT_DOMAIN}.
\$TTL 300

@       IN SOA  ns1.${ROUTEROOT_DOMAIN}. admin.${ROUTEROOT_DOMAIN}. (
                $(date +%Y%m%d%H)  ; serial
                3600        ; refresh
                900         ; retry
                604800      ; expire
                300         ; minimum TTL
)

@       IN NS   ns1.${ROUTEROOT_DOMAIN}.
@       IN NS   ns2.${ROUTEROOT_DOMAIN}.

ns1     IN A    ${ROUTEROOT_SERVER_IP}
ns2     IN A    ${ROUTEROOT_SERVER_IP}

@       IN A    ${ROUTEROOT_SERVER_IP}

*       IN A    ${ROUTEROOT_SERVER_IP}
EOF
    echo "  FIXED: Zone file created"
    FIXES=$((FIXES + 1))
else
    # Check zone file has correct IP
    if grep -q "IN A.*:" "coredns/zones/db.${ROUTEROOT_DOMAIN}" 2>/dev/null; then
        echo "  FIXING: Zone file has IPv6 in A records"
        sed -i "s/IN A    .*/IN A    ${ROUTEROOT_SERVER_IP}/g" "coredns/zones/db.${ROUTEROOT_DOMAIN}"
        echo "  FIXED: Zone IPs updated"
        FIXES=$((FIXES + 1))
    else
        echo "  OK: Zone file"
    fi
fi

# --- Check data dir ---
if [ ! -d data ]; then
    mkdir -p data
    echo "  FIXED: Created data directory"
    FIXES=$((FIXES + 1))
else
    echo "  OK: Data directory"
fi

# --- Check disk space ---
DISK_USAGE=$(df / --output=pcent 2>/dev/null | tail -1 | tr -d ' %' || echo "0")
if [ "$DISK_USAGE" -gt 90 ]; then
    echo "  WARN: Disk at ${DISK_USAGE}%"
    echo "  Cleaning up..."
    docker system prune -f 2>/dev/null || true
    apt-get clean 2>/dev/null || true
    DISK_USAGE=$(df / --output=pcent 2>/dev/null | tail -1 | tr -d ' %' || echo "0")
    echo "  Now at ${DISK_USAGE}%"
    FIXES=$((FIXES + 1))
else
    echo "  OK: Disk (${DISK_USAGE}%)"
fi

# --- Check container status ---
echo ""
RUNNING=$(docker compose ps --status running -q 2>/dev/null | wc -l || echo "0")
TOTAL=$(docker compose ps -q 2>/dev/null | wc -l || echo "0")
echo "  Containers: $RUNNING/$TOTAL running"

if [ "$RUNNING" -lt 3 ] || [ "$FIXES" -gt 0 ]; then
    echo ""
    if [ "$FIXES" -gt 0 ]; then
        echo "  Applied $FIXES fix(es). Restarting..."
    else
        echo "  Not all containers running. Restarting..."
    fi
    docker compose down 2>/dev/null || true
    docker compose up -d 2>&1
    echo ""
    echo "  Waiting for health..."
    for i in $(seq 1 30); do
        if curl -sf http://localhost:8053/api/health >/dev/null 2>&1; then
            echo ""
            echo "  OK: RouteRoot is healthy!"
            curl -sf http://localhost:8053/api/health 2>/dev/null | python3 -m json.tool 2>/dev/null || curl -sf http://localhost:8053/api/health
            echo ""
            exit 0
        fi
        printf "."
        sleep 3
    done
    echo ""
    echo "  WARN: API still not responding."
    echo "  Logs: docker compose logs --tail 20"
    exit 1
else
    echo ""
    HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" --max-time 5 http://localhost:8053/api/health 2>/dev/null || echo "000")
    if [ "$HTTP_CODE" = "200" ]; then
        echo "  OK: API healthy"
        curl -sf http://localhost:8053/api/health 2>/dev/null | python3 -m json.tool 2>/dev/null || curl -sf http://localhost:8053/api/health
    else
        echo "  WARN: API not responding (HTTP $HTTP_CODE)"
        echo "  Restarting..."
        docker compose down 2>/dev/null || true
        docker compose up -d
        sleep 5
        curl -sf http://localhost:8053/api/health 2>/dev/null || echo "  Still not up. Check: docker compose logs -f"
    fi
fi

echo ""
