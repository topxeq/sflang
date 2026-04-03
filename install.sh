#!/bin/bash
#
# Sflang Installation Script
# Downloads and installs the latest version of Sflang
#
# Usage: curl -fsSL https://raw.githubusercontent.com/topxeq/sflang/main/install.sh | bash
#

set -e

REPO="topxeq/sflang"
BINARY_NAME="sf"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
    exit 1
}

# Detect OS
detect_os() {
    case "$(uname -s)" in
        Darwin*)    echo "darwin" ;;
        Linux*)     echo "linux" ;;
        CYGWIN*|MINGW*|MSYS*)    echo "windows" ;;
        *)          error "Unsupported OS: $(uname -s)" ;;
    esac
}

# Detect architecture
detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)   echo "amd64" ;;
        arm64|aarch64)  echo "arm64" ;;
        *)              error "Unsupported architecture: $(uname -m)" ;;
    esac
}

# Get latest release version from GitHub API
get_latest_version() {
    local version
    version=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$version" ]; then
        error "Failed to get latest version"
    fi
    echo "$version"
}

# Get download URL for the current platform
get_download_url() {
    local os="$1"
    local arch="$2"
    local version="$3"

    local ext="tar.gz"
    if [ "$os" = "windows" ]; then
        ext="zip"
    fi

    echo "https://github.com/${REPO}/releases/download/${version}/sf-${os}-${arch}.${ext}"
}

# Install sflang
install() {
    local os
    local arch
    local version
    local url
    local install_dir
    local tmp_dir

    info "Detecting system..."
    os=$(detect_os)
    arch=$(detect_arch)
    info "System: ${os}/${arch}"

    info "Fetching latest version..."
    version=$(get_latest_version)
    info "Latest version: ${version}"

    # Determine install directory
    if [ -n "$SFLANG_INSTALL_DIR" ]; then
        install_dir="$SFLANG_INSTALL_DIR"
    elif [ -w "/usr/local/bin" ]; then
        install_dir="/usr/local/bin"
    else
        install_dir="${HOME}/.local/bin"
        mkdir -p "$install_dir"
    fi
    info "Install directory: ${install_dir}"

    # Create temp directory
    tmp_dir=$(mktemp -d)
    trap "rm -rf $tmp_dir" EXIT

    # Download
    url=$(get_download_url "$os" "$arch" "$version")
    info "Downloading: ${url}"

    local archive="${tmp_dir}/sflang.${os}-${arch}"
    if [ "$os" = "windows" ]; then
        archive="${archive}.zip"
    else
        archive="${archive}.tar.gz"
    fi

    curl -fsSL "$url" -o "$archive"

    # Extract
    info "Extracting..."
    cd "$tmp_dir"
    if [ "$os" = "windows" ]; then
        unzip -o "$archive" > /dev/null
    else
        tar -xzf "$archive"
    fi

    # Find and install binary
    local binary="${BINARY_NAME}"
    if [ "$os" = "windows" ]; then
        binary="${BINARY_NAME}.exe"
    fi

    # Look for binary in current dir or subdirectory
    local found_binary
    found_binary=$(find . -name "$binary" -type f | head -1)

    if [ -z "$found_binary" ]; then
        error "Binary not found in archive"
    fi

    # Make executable
    chmod +x "$found_binary"

    # Move to install directory
    mv "$found_binary" "${install_dir}/${binary}"

    info "Installed: ${install_dir}/${binary}"

    # Check if in PATH
    if ! echo "$PATH" | grep -q "$install_dir"; then
        warn "Install directory '${install_dir}' is not in PATH."
        echo ""
        echo "Add the following to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
        echo ""
        echo "    export PATH=\"\${PATH}:${install_dir}\""
        echo ""
        echo "Then run: source ~/.bashrc  (or ~/.zshrc)"
    fi

    # Verify installation
    if command -v sf &> /dev/null; then
        info "Verifying installation..."
        sf -version 2>/dev/null || info "sf installed successfully!"
    else
        info "Installation complete! Run '${install_dir}/sf' to use Sflang."
    fi

    echo ""
    info "Sflang ${version} installed successfully!"
}

# Run installation
install