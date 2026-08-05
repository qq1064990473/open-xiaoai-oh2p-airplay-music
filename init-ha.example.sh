#!/bin/sh

# Fail-open supervisor for the OH2P user-space client.
# A missing/invalid HA token leaves native XiaoAI NLP/TTS enabled.
set -u

CLIENT="/data/open-xiaoai/client-ha.new"
CONFIG="/data/open-xiaoai/client.controlfix.json"
SERVER="ws://192.168.31.200:4399"
PNS_LINK="/data/pns.lab"
PNS_RUNTIME="/tmp/open-xiaoai-pns.lab"
READY="/tmp/open-xiaoai-ha-ready"
LOG="/tmp/open-xiaoai-client.log"

: > "$LOG"
exec >> "$LOG" 2>&1

if [ -e "$PNS_LINK" ] && [ ! -L "$PNS_LINK" ]; then
    echo "[init] refusing to replace regular $PNS_LINK" >&2
    exit 1
fi
if [ ! -L "$PNS_LINK" ]; then
    ln -s "$PNS_RUNTIME" "$PNS_LINK" || exit 1
fi

child=""
lab_enabled=0

cleanup() {
    rm -f "$PNS_RUNTIME" "$READY"
    if [ "$lab_enabled" -eq 1 ]; then
        /etc/init.d/mico_aivs_lab restart >/dev/null 2>&1 || true
        lab_enabled=0
    fi
}

terminate() {
    if [ -n "$child" ]; then
        kill "$child" 2>/dev/null || true
        wait "$child" 2>/dev/null || true
    fi
    cleanup
    exit 0
}

trap terminate TERM INT

while :; do
    rm -f "$PNS_RUNTIME" "$READY"
    "$CLIENT" "$SERVER" -c "$CONFIG" &
    child=$!
    lab_enabled=0

    waited=0
    while kill -0 "$child" 2>/dev/null; do
        if [ -f "$READY" ] && [ ! -f "$PNS_RUNTIME" ]; then
            : > "$PNS_RUNTIME"
            lab_enabled=1
            /etc/init.d/mico_aivs_lab restart >/dev/null 2>&1 || true
            echo "[init] native ASR-only mode enabled"
            break
        fi
        waited=$((waited + 1))
        [ "$waited" -ge 30 ] && break
        sleep 1
    done

    wait "$child" 2>/dev/null || true
    child=""
    cleanup
    sleep 3
done
