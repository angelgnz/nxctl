#!/bin/sh
set -e

if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || true
    udevadm trigger -s hwmon || true
fi

if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || true
    systemctl --global enable nxctld.service || true
    if [ -n "${SUDO_USER:-}" ] && [ "${SUDO_USER}" != "root" ]; then
        systemctl --user -M "${SUDO_USER}@" daemon-reload || true
        systemctl --user -M "${SUDO_USER}@" enable --now nxctld.service || true
    fi
fi
