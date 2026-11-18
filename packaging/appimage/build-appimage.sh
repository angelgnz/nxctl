#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${1:-$(grep -m1 '^version = ' "${ROOT_DIR}/Cargo.toml" | cut -d'"' -f2)}"
TARGET_DIR="${2:-${ROOT_DIR}/target/release}"
OUT_DIR="${3:-${ROOT_DIR}/dist}"

for bin in nxctl nxctld nxctl-gui; do
  if [ ! -x "${TARGET_DIR}/${bin}" ]; then
    echo "error: missing release binary ${TARGET_DIR}/${bin}" >&2
    exit 1
  fi
done

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT
APP_DIR="${WORK_DIR}/nxctl.AppDir"

mkdir -p \
  "${APP_DIR}/usr/bin" \
  "${APP_DIR}/usr/share/applications" \
  "${APP_DIR}/usr/share/icons/hicolor/scalable/apps" \
  "${APP_DIR}/usr/share/icons/hicolor/scalable/status" \
  "${APP_DIR}/usr/share/nxctl/contrib"

install -Dm755 "${TARGET_DIR}/nxctl" "${APP_DIR}/usr/bin/nxctl"
install -Dm755 "${TARGET_DIR}/nxctld" "${APP_DIR}/usr/bin/nxctld"
install -Dm755 "${TARGET_DIR}/nxctl-gui" "${APP_DIR}/usr/bin/nxctl-gui"

install -Dm644 "${ROOT_DIR}/contrib/72-nzxt-hwmon.rules" \
  "${APP_DIR}/usr/share/nxctl/contrib/72-nzxt-hwmon.rules"
install -Dm644 "${ROOT_DIR}/contrib/nxctld.service" \
  "${APP_DIR}/usr/share/nxctl/contrib/nxctld.service"
install -Dm644 "${ROOT_DIR}/contrib/nxctld-user.service" \
  "${APP_DIR}/usr/share/nxctl/contrib/nxctld-user.service"
install -Dm755 "${ROOT_DIR}/contrib/nxctl-sleep" \
  "${APP_DIR}/usr/share/nxctl/contrib/nxctl-sleep"

install -Dm644 "${ROOT_DIR}/contrib/nxctl.svg" \
  "${APP_DIR}/usr/share/icons/hicolor/scalable/apps/nxctl.svg"
install -Dm644 "${ROOT_DIR}/contrib/nxctl-tray.svg" \
  "${APP_DIR}/usr/share/icons/hicolor/scalable/status/nxctl-tray.svg"
install -Dm644 "${ROOT_DIR}/contrib/nxctl.svg" "${APP_DIR}/nxctl.svg"
install -Dm644 "${ROOT_DIR}/crates/nxctl-gui/assets/icon.png" "${APP_DIR}/nxctl.png"
install -Dm644 "${ROOT_DIR}/crates/nxctl-gui/assets/icon.png" "${APP_DIR}/.DirIcon"

sed 's|^Exec=.*|Exec=nxctl-gui|' "${ROOT_DIR}/contrib/nxctl-gui.desktop" \
  > "${APP_DIR}/nxctl-gui.desktop"
install -Dm644 "${APP_DIR}/nxctl-gui.desktop" \
  "${APP_DIR}/usr/share/applications/nxctl-gui.desktop"

cat > "${APP_DIR}/AppRun" <<'EOF'
#!/bin/sh
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
export PATH="${HERE}/usr/bin:${PATH}"

CALLED="$(basename "$0")"
case "${CALLED}" in
  nxctl|nxctld|nxctl-gui)
    exec "${HERE}/usr/bin/${CALLED}" "$@"
    ;;
esac

case "${1:-}" in
  nxctl|nxctld|nxctl-gui)
    BIN="$1"
    shift
    exec "${HERE}/usr/bin/${BIN}" "$@"
    ;;
  *)
    exec "${HERE}/usr/bin/nxctl-gui" "$@"
    ;;
esac
EOF
chmod 0755 "${APP_DIR}/AppRun"

if command -v appimagetool >/dev/null 2>&1; then
  APPIMAGETOOL="$(command -v appimagetool)"
else
  APPIMAGETOOL="${WORK_DIR}/appimagetool"
  curl -fsSL \
    "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage" \
    -o "${APPIMAGETOOL}"
  chmod 0755 "${APPIMAGETOOL}"
fi

mkdir -p "${OUT_DIR}"
OUT_PATH="${OUT_DIR}/nxctl-${VERSION}-x86_64.AppImage"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "${APPIMAGETOOL}" "${APP_DIR}" "${OUT_PATH}"
chmod 0755 "${OUT_PATH}"
echo "Built ${OUT_PATH}"
