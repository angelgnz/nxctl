#!/bin/sh
set -e

if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || true
    udevadm trigger -s hwmon || true
fi

if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || true
fi
