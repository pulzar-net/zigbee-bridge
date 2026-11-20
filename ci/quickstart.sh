#!/usr/bin/env bash
# Runs the README's quick start as written, with the coordinator simulator in
# place of a USB coordinator: download the release and check its sum, then
# write the README's configuration, run the bridge with the README's command,
# pair with the README's command, and wait for the sensor's Home Assistant
# discovery message and its first reading. It runs twice: with the release
# binary the README downloads, and with this commit's build.
#
# Needs Linux, sudo, cargo, curl, jq, and mosquitto with its clients.
set -euo pipefail
cd "$(dirname "$0")/.."

# The body of the code block that follows `<!-- quickstart:NAME -->`.
block() {
    awk -v marker="<!-- quickstart:$1 -->" '
        $0 == marker { found = 1; next }
        found && /^```/ { if (inside) exit; inside = 1; next }
        inside { print }
    ' README.md
}

SIM_PORT=9999
SENSOR=0x00124b0000000001
work="$(mktemp -d)"
pids=()
cleanup() {
    for pid in "${pids[@]}"; do sudo kill "${pid}" 2>/dev/null || true; done
    rm -rf "${work}"
}
trap cleanup EXIT

echo "--- 1. Download, verify and install the release"
(cd "${work}" && bash -euxo pipefail -c "$(block download)")

echo "--- Build the simulator and this commit's bridge"
cargo build --locked -p zigbee-bridge -p zigbee-sim
build="$(pwd)/target/debug/zigbee-bridge"

# A fresh broker for each run, so no retained message from an earlier run
# can satisfy a check. The README's commands use the default port.
sudo systemctl stop mosquitto 2>/dev/null || true

# Wait up to $3 seconds for a message on topic $1 that the jq filter $2
# accepts. Retained messages count: the bridge may publish before we listen.
wait_for() {
    local deadline=$((SECONDS + $3))
    while [ "${SECONDS}" -lt "${deadline}" ]; do
        if mosquitto_sub -C 1 -W 5 -t "$1" 2>/dev/null | jq -e "$2" >/dev/null 2>&1; then
            return 0
        fi
    done
    echo "timed out waiting for $1 ($2)" >&2
    cat "${work}/bridge.log" >&2
    return 1
}

for bridge in zigbee-bridge "${build}"; do
    echo "=== quick start with ${bridge}"
    sudo rm -rf /var/lib/zigbee-bridge
    mosquitto -p 1883 >"${work}/broker.log" 2>&1 &
    pids+=("$!")
    target/debug/zigbee-ncp-sim --listen "127.0.0.1:${SIM_PORT}" --devices th --report-every 2 \
        >"${work}/sim.log" 2>&1 &
    pids+=("$!")
    sleep 1

    echo "--- 3. The README's configuration, with the simulator as the port"
    sudo mkdir -p /etc/zigbee-bridge
    block config \
        | sed "s|^port = .*|port = \"tcp://127.0.0.1:${SIM_PORT}\"|" \
        | sudo tee /etc/zigbee-bridge/config.toml

    echo "--- 4. Run"
    run="$(block run | sed "s|^sudo zigbee-bridge |sudo ${bridge} |")"
    bash -c "${run}" >"${work}/bridge.log" 2>&1 &
    wait_for zigbee/bridge/state '.state == "online"' 30

    echo "--- 5. Pair"
    bash -euxo pipefail -c "$(block pair)"
    wait_for "homeassistant/sensor/${SENSOR#0x}/temperature/config" \
        '.state_topic == "zigbee/'"${SENSOR}"'"' 90
    wait_for "zigbee/${SENSOR}" '.temperature | numbers' 60

    sudo pkill -f -- "zigbee-bridge --config" || true
    sleep 1
    for pid in "${pids[@]}"; do kill "${pid}" 2>/dev/null || true; done
    pids=()
    sleep 1
done
echo "quick start: ok"
