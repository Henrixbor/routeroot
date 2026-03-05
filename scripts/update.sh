#!/usr/bin/env bash
set -euo pipefail

# AgentDNS — Pull latest and rebuild
INSTALL_DIR="${AGENTDNS_DIR:-/opt/agentdns}"

cd "$INSTALL_DIR"
echo "Pulling latest..."
git pull --ff-only

echo "Rebuilding and restarting..."
docker compose up -d --build --remove-orphans

echo "Waiting for health..."
for i in $(seq 1 20); do
    if curl -sf http://localhost:8053/api/health >/dev/null 2>&1; then
        echo "AgentDNS updated and healthy."
        exit 0
    fi
    sleep 3
done

echo "Warning: health check not passing yet. Check: docker compose logs -f"
