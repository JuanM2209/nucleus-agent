#!/bin/bash
# Nucleus Agent vr23 — One-line installer for N-1065 Tyrion boards
# Usage: curl -fsSL https://raw.githubusercontent.com/JuanM2209/nucleus-agent/main/install.sh | bash
set -e

IMAGE_NAME="nucleus-agent:vr23"
RELEASE_URL="https://github.com/JuanM2209/nucleus-agent/releases/download/vr23/nucleus-agent-vr23.tar.gz"
CONTAINER_NAME="remote-s"
SERVER_URL="${AGENT_SERVER_URL:-wss://api.datadesng.com/ws/agent}"

# Auto-detect device ID from factory serial
DEVICE_ID="${AGENT_TOKEN:-$(cat /data/nucleus/factory/nucleus_serial_number 2>/dev/null)}"

echo "╔═══════════════════════════════════════╗"
echo "║   Nucleus Agent vr23 Installer        ║"
echo "╚═══════════════════════════════════════╝"
echo ""
echo "Device:  ${DEVICE_ID:-UNKNOWN}"
echo "Server:  ${SERVER_URL}"
echo "Image:   ${IMAGE_NAME}"
echo ""

if [ -z "$DEVICE_ID" ]; then
  echo "ERROR: No device ID found."
  echo "Set AGENT_TOKEN or check /data/nucleus/factory/nucleus_serial_number"
  exit 1
fi

echo "[1/4] Downloading agent image..."
curl -fsSL "${RELEASE_URL}" -o /tmp/nucleus-agent-vr23.tar.gz
docker load < /tmp/nucleus-agent-vr23.tar.gz
rm -f /tmp/nucleus-agent-vr23.tar.gz

echo "[2/4] Stopping old container..."
docker stop "${CONTAINER_NAME}" 2>/dev/null || true
docker rm "${CONTAINER_NAME}" 2>/dev/null || true

echo "[3/4] Starting agent vr23..."
docker run -d \
  --name "${CONTAINER_NAME}" \
  --restart unless-stopped \
  --network host \
  --cap-add=SYS_ADMIN \
  --pid=host \
  -v /data/nucleus:/data/nucleus:ro \
  -v /dev:/dev \
  -v /var/run/dbus:/var/run/dbus \
  -v /sys/class/net:/sys/class/net:ro \
  -e AGENT_SERVER_URL="${SERVER_URL}" \
  -e AGENT_TOKEN="${DEVICE_ID}" \
  "${IMAGE_NAME}"

echo ""
echo "[4/4] Verifying..."
sleep 2
STATUS=$(docker inspect -f '{{.State.Status}}' ${CONTAINER_NAME} 2>/dev/null || echo "error")
echo ""
echo "╔═══════════════════════════════════════╗"
echo "║   Agent vr23 deployed: ${STATUS}         ║"
echo "╚═══════════════════════════════════════╝"
echo "Logs: docker logs -f ${CONTAINER_NAME}"
