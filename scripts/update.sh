#!/usr/bin/env bash
set -euo pipefail

# AgentDNS — Pull latest and rebuild
# Usage: sudo bash scripts/update.sh

INSTALL_DIR="${AGENTDNS_DIR:-/opt/agentdns}"
cd "$INSTALL_DIR" || { echo "FAIL: $INSTALL_DIR not found"; exit 1; }

echo "Pulling latest..."
git pull --ff-only

echo "Rebuilding..."
docker compose build
docker compose up -d --remove-orphans

echo "Waiting for health..."
for i in $(seq 1 30); do
    if curl -sf http://localhost:8053/api/health >/dev/null 2>&1; then
        echo "AgentDNS updated and healthy."
        curl -sf http://localhost:8053/api/health
        echo ""
        exit 0
    fi
    sleep 3
done

echo "Not healthy yet. Check: docker compose logs -f"
