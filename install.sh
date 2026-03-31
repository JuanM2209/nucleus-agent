#!/bin/bash
# Nucleus Agent vr23 — One-line installer for ARM devices (N-1065 Tyrion boards)
# Usage: curl -fsSL https://raw.githubusercontent.com/JuanM2209/nucleus-portal/master/scripts/install-agent.sh | bash
#
# Environment variables (set before running):
#   AGENT_TOKEN     — Device UUID (required if not using default)
#   AGENT_SERVER_URL — WebSocket endpoint (default: wss://api.datadesng.com/ws/agent)
set -e

AGENT_IMAGE="ghcr.io/juanm2209/nucleus-agent:vr23"
CONTAINER_NAME="nucleus-agent"
DEVICE_ID="${AGENT_TOKEN:-}"
SERVER_URL="${AGENT_SERVER_URL:-wss://api.datadesng.com/ws/agent}"

echo "╔═══════════════════════════════════════╗"
echo "║   Nucleus Agent vr23 Installer        ║"
echo "╚═══════════════════════════════════════╝"
echo ""

# Auto-detect device ID from factory serial if not provided
if [ -z "$DEVICE_ID" ]; then
  if [ -f /data/nucleus/factory/nucleus_serial_number ]; then
    DEVICE_ID=$(cat /data/nucleus/factory/nucleus_serial_number)
    echo "Auto-detected device: ${DEVICE_ID}"
  else
    echo "ERROR: No AGENT_TOKEN set and no factory serial found."
    echo "Usage: AGENT_TOKEN=<device-uuid> bash install-agent.sh"
    exit 1
  fi
fi

echo "Device:  ${DEVICE_ID}"
echo "Server:  ${SERVER_URL}"
echo "Image:   ${AGENT_IMAGE}"
echo ""

# Detect architecture
ARCH=$(uname -m)
echo "[1/5] Architecture: ${ARCH}"
if [ "$ARCH" != "armv7l" ] && [ "$ARCH" != "aarch64" ] && [ "$ARCH" != "x86_64" ]; then
  echo "WARNING: Unsupported architecture ${ARCH}. Image is built for armv7l."
fi

# Pull image (public registry, no auth needed)
echo "[2/5] Pulling ${AGENT_IMAGE}..."
docker pull "${AGENT_IMAGE}" || {
  echo "Pull failed. Trying with authentication..."
  echo "Please run: docker login ghcr.io"
  exit 1
}

# Stop old agent (any version)
echo "[3/5] Stopping old agent..."
docker stop "${CONTAINER_NAME}" 2>/dev/null || true
docker rm "${CONTAINER_NAME}" 2>/dev/null || true

# Create config directory
echo "[4/5] Preparing config..."
mkdir -p /etc/nucleus

# Start new agent
echo "[5/5] Starting agent vr23..."
docker run -d \
  --name "${CONTAINER_NAME}" \
  --restart unless-stopped \
  --network host \
  --cap-add=SYS_ADMIN \
  --pid=host \
  -v /etc/nucleus:/etc/nucleus \
  -v /var/run/dbus:/var/run/dbus \
  -v /sys/class/net:/sys/class/net:ro \
  -e AGENT_SERVER_URL="${SERVER_URL}" \
  -e AGENT_TOKEN="${DEVICE_ID}" \
  "${AGENT_IMAGE}"

echo ""
echo "╔═══════════════════════════════════════╗"
echo "║   Agent vr23 deployed successfully    ║"
echo "╚═══════════════════════════════════════╝"
echo "Status:  $(docker inspect -f '{{.State.Status}}' ${CONTAINER_NAME} 2>/dev/null)"
echo "Logs:    docker logs -f ${CONTAINER_NAME}"
echo "Update:  curl -fsSL https://raw.githubusercontent.com/JuanM2209/nucleus-portal/master/scripts/install-agent.sh | bash"
