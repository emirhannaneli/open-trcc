#!/usr/bin/env bash
# Open TRCC installer for any Linux distribution.
#
#   curl -fsSL https://raw.githubusercontent.com/emirhannaneli/open-trcc/main/install.sh | bash
#   ./install.sh --uninstall
#
# Installs the latest AppImage for the current user (no root), adds a menu entry,
# and installs a udev rule (needs sudo once) so the cooler works without root.
# The app enables "start on login" itself on first launch.
set -euo pipefail

REPO="emirhannaneli/open-trcc"
APP="open-trcc"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
INSTALL_DIR="$DATA_HOME/$APP"
BIN_DIR="$HOME/.local/bin"
APPIMAGE="$INSTALL_DIR/$APP.AppImage"
DESKTOP_FILE="$DATA_HOME/applications/$APP.desktop"
ICON_FILE="$DATA_HOME/icons/hicolor/128x128/apps/$APP.png"
UDEV_RULE="/etc/udev/rules.d/70-open-trcc.rules"

info() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m==>\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31m==>\033[0m %s\n' "$*" >&2; exit 1; }

fetch() { # fetch URL DEST
  if command -v curl >/dev/null 2>&1; then
    curl -fL --progress-bar "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q --show-progress "$1" -O "$2"
  else
    die "curl or wget is required"
  fi
}

fetch_text() {
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1"; else wget -qO- "$1"; fi
}

# The AppImage runtime and the app itself are separate processes; stop both,
# or the single-instance lock keeps the old version running after an update.
stop_running() {
  pkill -f "$APPIMAGE" 2>/dev/null || true
  pkill -x "$APP" 2>/dev/null || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    pgrep -x "$APP" >/dev/null 2>&1 || return 0
    sleep 0.5
  done
  pkill -9 -x "$APP" 2>/dev/null || true
}

as_root() {
  if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo "$@"; fi
}

uninstall() {
  info "Stopping Open TRCC"
  stop_running
  info "Removing files"
  rm -f "$APPIMAGE" "$BIN_DIR/$APP" "$DESKTOP_FILE" "$ICON_FILE"
  rm -f "$CONFIG_HOME/autostart/$APP.desktop" "$CONFIG_HOME"/autostart/open-trcc*.desktop
  rmdir "$INSTALL_DIR" 2>/dev/null || true
  if [ -f "$UDEV_RULE" ]; then
    info "Removing udev rule (sudo)"
    as_root rm -f "$UDEV_RULE"
    as_root udevadm control --reload-rules || true
  fi
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$DATA_HOME/applications" || true
  info "Open TRCC removed (settings kept in $CONFIG_HOME/com.emirman.open-trcc)"
}

has_fuse2() {
  ldconfig -p 2>/dev/null | grep -q 'libfuse\.so\.2' && return 0
  ls /usr/lib*/libfuse.so.2 /usr/lib/*/libfuse.so.2 >/dev/null 2>&1
}

ensure_fuse2() {
  has_fuse2 && return 0
  warn "AppImages need FUSE 2 (libfuse.so.2), which is not installed."
  local cmd=""
  if command -v pacman >/dev/null 2>&1; then cmd="pacman -S --needed --noconfirm fuse2"
  elif command -v apt-get >/dev/null 2>&1; then
    if apt-cache show libfuse2t64 >/dev/null 2>&1; then cmd="apt-get install -y libfuse2t64"
    else cmd="apt-get install -y libfuse2"; fi
  elif command -v dnf >/dev/null 2>&1; then cmd="dnf install -y fuse-libs"
  elif command -v zypper >/dev/null 2>&1; then cmd="zypper --non-interactive install libfuse2"
  elif command -v xbps-install >/dev/null 2>&1; then cmd="xbps-install -y fuse"
  elif command -v apk >/dev/null 2>&1; then cmd="apk add fuse"
  fi
  if [ -n "$cmd" ]; then
    info "Installing FUSE 2: $cmd (sudo)"
    # shellcheck disable=SC2086
    as_root $cmd || warn "Could not install FUSE 2; install it manually."
  else
    warn "Install FUSE 2 with your package manager, then run open-trcc."
  fi
}

install_udev_rule() {
  local tag="$1" tmp
  tmp="$(mktemp)"
  local rule_path="src-tauri/linux/70-open-trcc.rules"
  fetch_text "https://raw.githubusercontent.com/$REPO/$tag/$rule_path" >"$tmp" 2>/dev/null \
    || fetch_text "https://raw.githubusercontent.com/$REPO/main/$rule_path" >"$tmp" \
    || die "Could not download the udev rule"
  if [ -f "$UDEV_RULE" ] && cmp -s "$tmp" "$UDEV_RULE"; then
    rm -f "$tmp"
    return 0
  fi
  info "Installing udev rule so the cooler works without root (sudo)"
  as_root install -Dm644 "$tmp" "$UDEV_RULE"
  rm -f "$tmp"
  as_root udevadm control --reload-rules
  as_root udevadm trigger --subsystem-match=hidraw --subsystem-match=usb || true
}

install_app() {
  [ "$(uname -s)" = "Linux" ] || die "This installer is for Linux. Use the .exe/.msi or .dmg from the releases page."
  [ "$(uname -m)" = "x86_64" ] || die "Only x86_64 AppImages are published right now."

  info "Looking up the latest release"
  local release tag url
  release="$(fetch_text "https://api.github.com/repos/$REPO/releases/latest")" \
    || die "Could not reach GitHub"
  tag="$(printf '%s' "$release" | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
  url="$(printf '%s' "$release" | grep -o '"browser_download_url": *"[^"]*_amd64\.AppImage"' \
    | head -n1 | sed -E 's/.*"(https[^"]+)"/\1/')"
  [ -n "$tag" ] && [ -n "$url" ] || die "No AppImage found in the latest release"

  info "Downloading $tag"
  mkdir -p "$INSTALL_DIR" "$BIN_DIR" "$(dirname "$DESKTOP_FILE")" "$(dirname "$ICON_FILE")"
  fetch "$url" "$APPIMAGE.part"
  chmod +x "$APPIMAGE.part"
  stop_running
  mv -f "$APPIMAGE.part" "$APPIMAGE"
  ln -sf "$APPIMAGE" "$BIN_DIR/$APP"

  fetch_text "https://raw.githubusercontent.com/$REPO/$tag/src-tauri/icons/128x128.png" >"$ICON_FILE" || true

  cat >"$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=Open TRCC
GenericName=Cooler Control Center
Comment=Control Thermalright cooler displays
Exec=$APPIMAGE
Icon=$APP
Terminal=false
Categories=Utility;System;
StartupWMClass=open-trcc
EOF
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$DATA_HOME/applications" || true

  ensure_fuse2
  install_udev_rule "$tag"

  info "Starting Open TRCC (it will also start on every login)"
  nohup "$APPIMAGE" >/dev/null 2>&1 &
  disown || true

  info "Installed $tag. Find \"Open TRCC\" in your app menu; it lives in the system tray."
  case ":$PATH:" in *":$BIN_DIR:"*) ;; *) warn "$BIN_DIR is not in PATH; launch it from the app menu instead." ;; esac
}

case "${1:-}" in
  --uninstall|-u) uninstall ;;
  ""|--install) install_app ;;
  *) die "Usage: $0 [--uninstall]" ;;
esac
