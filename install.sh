#!/bin/bash
# Nucleus Agent vr23 — One-line installer for ARM devices (N-1065 Tyrion boards)
# Usage: curl -fsSL https://raw.githubusercontent.com/JuanM2209/nucleus-agent/main/install.sh | bash
set -e

IMAGE="ghcr.io/juanm2209/nucleus-agent:vr23"
RELEASE_URL="https://github.com/JuanM2209/nucleus-agent/releases/download/vr23/nucleus-agent-vr23.tar.gz"
CONTAINER_NAME="nucleus-agent"
DEVICE_ID="${AGENT_TOKEN:-}"
SERVER_URL="${AGENT_SERVER_URL:-wss://api.datadesng.com/ws/agent}"

echo "╔═══════════════════════════════════════╗"
echo "║   Nucleus Agent vr23 Installer        ║"
echo "╚═══════════════════════════════════════╝"
echo ""

# Auto-detect device ID from factory serial
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
echo "Image:   ${IMAGE}"
echo ""

echo "[1/4] Downloading agent image..."
curl -fsSL "${RELEASE_URL}" -o /tmp/nucleus-agent-vr23.tar.gz
docker load < /tmp/nucleus-agent-vr23.tar.gz
rm -f /tmp/nucleus-agent-vr23.tar.gz

echo "[2/4] Stopping old agent..."
docker stop "${CONTAINER_NAME}" 2>/dev/null || true
docker rm "${CONTAINER_NAME}" 2>/dev/null || true

echo "[3/4] Preparing config..."
mkdir -p /etc/nucleus

echo "[4/4] Starting agent vr23..."
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
  "${IMAGE}"

echo ""
echo "╔═══════════════════════════════════════╗"
echo "║   Agent vr23 deployed successfully    ║"
echo "╚═══════════════════════════════════════╝"
echo "Status:  $(docker inspect -f '{{.State.Status}}' ${CONTAINER_NAME} 2>/dev/null)"
echo "Logs:    docker logs -f ${CONTAINER_NAME}"
