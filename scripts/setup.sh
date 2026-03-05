#!/usr/bin/env bash
set -euo pipefail

# AgentDNS Server Setup Script
# Run on a fresh Ubuntu/Debian server

echo "=== AgentDNS Server Setup ==="

# Install Docker if not present
if ! command -v docker &> /dev/null; then
    echo "Installing Docker..."
    curl -fsSL https://get.docker.com | sh
    systemctl enable docker
    systemctl start docker
fi

# Install Docker Compose plugin if not present
if ! docker compose version &> /dev/null; then
    echo "Installing Docker Compose plugin..."
    apt-get update && apt-get install -y docker-compose-plugin
fi

# Open required ports (if ufw is active)
if command -v ufw &> /dev/null && ufw status | grep -q "active"; then
    echo "Configuring firewall..."
    ufw allow 53/udp   # DNS
    ufw allow 53/tcp   # DNS
    ufw allow 80/tcp   # HTTP
    ufw allow 443/tcp  # HTTPS
    ufw allow 8053/tcp # API
fi

# Create data directory
mkdir -p data

# Generate API key if not set
if [ ! -f .env ]; then
    API_KEY=$(openssl rand -hex 32)
    cat > .env <<EOF
AGENTDNS_DOMAIN=agentdns.dev
AGENTDNS_SERVER_IP=$(curl -s ifconfig.me)
AGENTDNS_API_KEY=${API_KEY}
AGENTDNS_MAX_DEPLOYMENTS=20
AGENTDNS_DEFAULT_TTL=48h
AGENTDNS_MAX_MEMORY=2048
AGENTDNS_MAX_CPUS=2
EOF
    echo "Generated .env with API key: ${API_KEY}"
    echo "IMPORTANT: Update AGENTDNS_DOMAIN and AGENTDNS_SERVER_IP in .env"
fi

echo ""
echo "=== Setup Complete ==="
echo "Next steps:"
echo "  1. Edit .env with your domain and server IP"
echo "  2. Point your domain's NS records to this server's IP"
echo "  3. Run: docker compose up -d"
echo "  4. Test: curl http://localhost:8053/api/health"
