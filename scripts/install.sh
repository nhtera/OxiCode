#!/usr/bin/env bash
# OxiCode installer — detects platform, downloads latest release, installs binary.
# Usage: curl -fsSL https://raw.githubusercontent.com/nicktien007/oxicode/main/scripts/install.sh | bash

set -euo pipefail

REPO="nicktien007/oxicode"
INSTALL_DIR="${OXICODE_INSTALL_DIR:-$HOME/.local/bin}"
BINARY_NAME="oxicode"

info() { printf "\033[1;34m[info]\033[0m %s\n" "$1"; }
error() { printf "\033[1;31m[error]\033[0m %s\n" "$1" >&2; exit 1; }
success() { printf "\033[1;32m[ok]\033[0m %s\n" "$1"; }

# Detect platform
detect_platform() {
    local os arch target

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)  os="unknown-linux-gnu" ;;
        Darwin) os="apple-darwin" ;;
        *)      error "Unsupported OS: $os" ;;
    esac

    case "$arch" in
        x86_64|amd64)   arch="x86_64" ;;
        aarch64|arm64)  arch="aarch64" ;;
        *)              error "Unsupported architecture: $arch" ;;
    esac

    target="${arch}-${os}"
    echo "$target"
}

# Get latest release tag from GitHub
get_latest_version() {
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
        | grep '"tag_name"' \
        | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/'
}

main() {
    info "Installing OxiCode..."

    local target version url archive checksum_url tmp_dir

    target="$(detect_platform)"
    info "Detected platform: $target"

    version="$(get_latest_version 2>/dev/null || echo "")"
    if [ -z "$version" ]; then
        error "Could not determine latest version. Check https://github.com/${REPO}/releases"
    fi
    info "Latest version: $version"

    archive="${BINARY_NAME}-${version}-${target}.tar.gz"
    url="https://github.com/${REPO}/releases/download/${version}/${archive}"
    checksum_url="${url}.sha256"

    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' EXIT

    info "Downloading ${url}..."
    curl -fsSL "$url" -o "${tmp_dir}/${archive}" || error "Download failed"

    # Verify checksum if sha256sum is available
    if command -v sha256sum &>/dev/null; then
        info "Verifying checksum..."
        curl -fsSL "$checksum_url" -o "${tmp_dir}/${archive}.sha256" 2>/dev/null
        if [ -f "${tmp_dir}/${archive}.sha256" ]; then
            (cd "$tmp_dir" && sha256sum -c "${archive}.sha256") || error "Checksum verification failed"
            success "Checksum verified"
        fi
    elif command -v shasum &>/dev/null; then
        info "Verifying checksum..."
        curl -fsSL "$checksum_url" -o "${tmp_dir}/${archive}.sha256" 2>/dev/null
        if [ -f "${tmp_dir}/${archive}.sha256" ]; then
            expected="$(awk '{print $1}' "${tmp_dir}/${archive}.sha256")"
            actual="$(shasum -a 256 "${tmp_dir}/${archive}" | awk '{print $1}')"
            [ "$expected" = "$actual" ] || error "Checksum mismatch"
            success "Checksum verified"
        fi
    fi

    info "Extracting..."
    tar xzf "${tmp_dir}/${archive}" -C "$tmp_dir"

    info "Installing to ${INSTALL_DIR}..."
    mkdir -p "$INSTALL_DIR"
    mv "${tmp_dir}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
    chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

    # Check if INSTALL_DIR is in PATH
    if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
        echo ""
        info "Add ${INSTALL_DIR} to your PATH:"
        echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
        echo ""
    fi

    success "OxiCode ${version} installed to ${INSTALL_DIR}/${BINARY_NAME}"
    "${INSTALL_DIR}/${BINARY_NAME}" --version 2>/dev/null || true
}

main "$@"
