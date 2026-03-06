#!/usr/bin/env bash
set -euo pipefail

# AgentDNS Doctor — diagnose and auto-fix common issues
# Usage: sudo bash scripts/doctor.sh

INSTALL_DIR="${AGENTDNS_DIR:-/opt/agentdns}"
FIXES=0

echo ""
echo "  AgentDNS Doctor"
echo "  ==============="
echo ""

cd "$INSTALL_DIR" 2>/dev/null || { echo "  FAIL: $INSTALL_DIR not found. Run setup.sh first."; exit 1; }

# --- Check .env ---
if [ ! -f .env ]; then
    echo "  FAIL: .env missing"
    exit 1
else
    source .env
    echo "  OK: .env found (domain=$AGENTDNS_DOMAIN)"
fi

# --- Check systemd-resolved conflict ---
if ss -tlnp 2>/dev/null | grep -q "systemd-resolve.*:53"; then
    echo "  FIXING: systemd-resolved is using port 53"
    systemctl stop systemd-resolved 2>/dev/null || true
    systemctl disable systemd-resolved 2>/dev/null || true
    if [ ! -f /etc/resolv.conf.bak ]; then
        cp /etc/resolv.conf /etc/resolv.conf.bak 2>/dev/null || true
    fi
    echo "nameserver 8.8.8.8" > /etc/resolv.conf
    echo "nameserver 1.1.1.1" >> /etc/resolv.conf
    echo "  FIXED: systemd-resolved disabled, using 8.8.8.8"
    FIXES=$((FIXES + 1))
elif ss -tlnp 2>/dev/null | grep -v docker | grep -q ":53 "; then
    echo "  WARN: Something other than Docker is using port 53"
    ss -tlnp | grep ":53 "
else
    echo "  OK: Port 53 available"
fi

# --- Check Corefile has actual domain (not env var placeholder) ---
if grep -q '{\$AGENTDNS_DOMAIN' coredns/Corefile 2>/dev/null; then
    echo "  FIXING: Corefile has env var placeholder (CoreDNS doesn't support this)"
    cat > coredns/Corefile <<EOF
.:53 {
    file /etc/coredns/zones/db.${AGENTDNS_DOMAIN} ${AGENTDNS_DOMAIN}
    reload 5s
    log
    errors
    health :8054
    ready :8055
}
EOF
    echo "  FIXED: Corefile written with domain=$AGENTDNS_DOMAIN"
    FIXES=$((FIXES + 1))
else
    echo "  OK: Corefile"
fi

# --- Check zone file exists ---
if [ ! -f "coredns/zones/db.${AGENTDNS_DOMAIN}" ]; then
    echo "  FIXING: Zone file missing for $AGENTDNS_DOMAIN"
    mkdir -p coredns/zones
    cat > "coredns/zones/db.${AGENTDNS_DOMAIN}" <<EOF
\$ORIGIN ${AGENTDNS_DOMAIN}.
\$TTL 300

@       IN SOA  ns1.${AGENTDNS_DOMAIN}. admin.${AGENTDNS_DOMAIN}. (
                $(date +%Y%m%d%H)  ; serial
                3600        ; refresh
                900         ; retry
                604800      ; expire
                300         ; minimum TTL
)

@       IN NS   ns1.${AGENTDNS_DOMAIN}.
@       IN NS   ns2.${AGENTDNS_DOMAIN}.

ns1     IN A    ${AGENTDNS_SERVER_IP}
ns2     IN A    ${AGENTDNS_SERVER_IP}

@       IN A    ${AGENTDNS_SERVER_IP}

*       IN A    ${AGENTDNS_SERVER_IP}
EOF
    echo "  FIXED: Zone file created"
    FIXES=$((FIXES + 1))
else
    echo "  OK: Zone file exists"
fi

# --- Check zone file has IPv4 not IPv6 ---
if grep -q "IN A.*:" "coredns/zones/db.${AGENTDNS_DOMAIN}" 2>/dev/null; then
    echo "  FIXING: Zone file has IPv6 in A records (need AAAA for IPv6)"
    sed -i "s/IN A.*/IN A    ${AGENTDNS_SERVER_IP}/g" "coredns/zones/db.${AGENTDNS_DOMAIN}"
    echo "  FIXED: Zone file updated with IPv4 $AGENTDNS_SERVER_IP"
    FIXES=$((FIXES + 1))
else
    echo "  OK: Zone file IPs"
fi

# --- Check .env has IPv4 not IPv6 ---
if echo "$AGENTDNS_SERVER_IP" | grep -q ":"; then
    REAL_IP=$(curl -4 -sf --max-time 5 ifconfig.me 2>/dev/null || ip -4 addr show eth0 2>/dev/null | grep -oP 'inet \K[\d.]+' | head -1 || echo "")
    if [ -n "$REAL_IP" ]; then
        echo "  FIXING: .env has IPv6 ($AGENTDNS_SERVER_IP), switching to IPv4 ($REAL_IP)"
        sed -i "s/AGENTDNS_SERVER_IP=.*/AGENTDNS_SERVER_IP=$REAL_IP/" .env
        sed -i "s/$AGENTDNS_SERVER_IP/$REAL_IP/g" "coredns/zones/db.${AGENTDNS_DOMAIN}"
        echo "  FIXED: Updated to $REAL_IP"
        FIXES=$((FIXES + 1))
    else
        echo "  WARN: .env has IPv6 but could not detect IPv4"
    fi
else
    echo "  OK: Server IP is IPv4"
fi

# --- Check Docker ---
if ! command -v docker &>/dev/null; then
    echo "  FAIL: Docker not installed"
    exit 1
else
    echo "  OK: Docker installed"
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
    echo "  WARN: Disk usage at ${DISK_USAGE}% — consider freeing space"
    echo "        docker system prune -a  (removes unused images)"
else
    echo "  OK: Disk space (${DISK_USAGE}% used)"
fi

# --- Restart services if fixes were applied ---
if [ "$FIXES" -gt 0 ]; then
    echo ""
    echo "  Applied $FIXES fix(es). Restarting services..."
    docker compose down 2>/dev/null || true
    docker compose up -d
    echo ""
    echo "  Waiting for health..."
    for i in $(seq 1 20); do
        if curl -sf http://localhost:8053/api/health >/dev/null 2>&1; then
            echo "  OK: AgentDNS is healthy!"
            echo ""
            exit 0
        fi
        printf "."
        sleep 3
    done
    echo ""
    echo "  WARN: API not responding yet. Check: docker compose logs -f"
else
    # Just check health
    echo ""
    HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" --max-time 5 http://localhost:8053/api/health 2>/dev/null || echo "000")
    if [ "$HTTP_CODE" = "200" ]; then
        echo "  OK: API healthy (HTTP 200)"
    else
        echo "  WARN: API not responding (HTTP $HTTP_CODE)"
        echo "        Try: docker compose up -d"
    fi
fi

echo ""
