#!/usr/bin/env bash
set -euo pipefail

BR=br0
TAP=tap0
SUBNET=192.168.100.0/24
BR_IP=192.168.100.1/24

# Detect default outbound interface
DEF_IF=$(ip route | awk '/^default/ {print $5; exit}')
if [ -z "${DEF_IF}" ]; then
  echo "No default route interface found" >&2
  exit 1
fi

echo "Using outbound interface: ${DEF_IF}"

# Create bridge if missing
if ! ip link show "$BR" >/dev/null 2>&1; then
  sudo ip link add "$BR" type bridge
fi

# Assign bridge IP if missing
if ! ip addr show "$BR" | grep -q "${BR_IP%/*}"; then
  sudo ip addr add "$BR_IP" dev "$BR"
fi

# Bring bridge up
sudo ip link set "$BR" up

# Create tap if missing
if ! ip link show "$TAP" >/dev/null 2>&1; then
  sudo ip tuntap add dev "$TAP" mode tap user "$USER"
fi

# Attach tap to bridge
sudo ip link set "$TAP" master "$BR"
sudo ip link set "$TAP" up

# Enable IP forwarding
sudo sysctl -w net.ipv4.ip_forward=1

# Add NAT rule if missing
if ! sudo iptables -t nat -C POSTROUTING -s "$SUBNET" -o "$DEF_IF" -j MASQUERADE 2>/dev/null; then
  sudo iptables -t nat -A POSTROUTING -s "$SUBNET" -o "$DEF_IF" -j MASQUERADE
fi

echo "Bridge ${BR} up, tap ${TAP} up, NAT enabled for ${SUBNET} -> ${DEF_IF}"

echo "Guest IP suggestion: 192.168.100.2/24, gateway 192.168.100.1"
