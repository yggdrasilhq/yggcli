#!/usr/bin/env bash
set -euo pipefail

REPO="${YGGCLI_REPO:-https://github.com/yggdrasilhq/yggcli}"
VERSION="${VERSION:-latest}"
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

log() {
  printf '[yggcli-install] %s\n' "$*"
}

detect_platform() {
  local uname_s uname_m
  uname_s="$(uname -s)"
  uname_m="$(uname -m)"

  case "$uname_s" in
    Linux) OS="linux" ;;
    Android) OS="android" ;;
    *)
      if [[ "${PREFIX:-}" == *com.termux* ]]; then
        OS="android"
      else
        log "unsupported OS: $uname_s"
        exit 1
      fi
      ;;
  esac

  case "$uname_m" in
    x86_64|amd64) ARCH="amd64" ;;
    aarch64|arm64) ARCH="arm64" ;;
    *)
      log "unsupported architecture: $uname_m"
      exit 1
      ;;
  esac

  if [[ "$OS" == "android" && -n "${PREFIX:-}" && -w "${PREFIX}/bin" ]]; then
    INSTALL_DIR="${INSTALL_DIR:-${PREFIX}/bin}"
  else
    INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
  fi
}

install_from_release() {
  local version asset url out
  version="$1"
  asset="yggcli-${OS}-${ARCH}"
  if [[ "$version" == "latest" ]]; then
    url="${REPO%/}/releases/latest/download/${asset}"
  else
    url="${REPO%/}/releases/download/${version}/${asset}"
  fi
  out="$TMPDIR/yggcli"
  log "downloading ${url}"
  curl -fsSL "$url" -o "$out"
  chmod +x "$out"
  mkdir -p "$INSTALL_DIR"
  install -m 0755 "$out" "$INSTALL_DIR/yggcli"
}

build_from_source() {
  local clone_dir
  clone_dir="$TMPDIR/yggcli-src"
  log "release asset unavailable; building from source"

  if [[ "$OS" == "android" ]]; then
    if ! command -v pkg >/dev/null 2>&1; then
      log "Termux pkg not found; cannot bootstrap Android build fallback"
      exit 1
    fi
    pkg install -y git rust >/dev/null
  fi

  command -v git >/dev/null 2>&1 || { log "git is required for source fallback"; exit 1; }
  command -v cargo >/dev/null 2>&1 || { log "cargo is required for source fallback"; exit 1; }

  git clone --depth 1 "${REPO%/}.git" "$clone_dir" >/dev/null 2>&1
  (
    cd "$clone_dir"
    cargo build --release >/dev/null
  )
  mkdir -p "$INSTALL_DIR"
  install -m 0755 "$clone_dir/target/release/yggcli" "$INSTALL_DIR/yggcli"
}

detect_platform

if ! install_from_release "$VERSION"; then
  build_from_source
fi

log "installed to $INSTALL_DIR/yggcli"
log "run: yggcli --help"
