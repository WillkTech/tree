#!/usr/bin/env bash
# tree installer script
# Usage: curl -sSfL https://raw.githubusercontent.com/willktech/tree/master/install.sh | sh

set -e

REPO="willktech/tree"
BINARY_NAME="tree"

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
        Linux*)     OS="linux" ;;
        Darwin*)    OS="darwin" ;;
        MINGW*|MSYS*|CYGWIN*) OS="windows" ;;
        *)          error "Unsupported operating system: $(uname -s)" ;;
    esac
}

# Detect architecture
detect_arch() {
    ARCH=$(uname -m)
    case "$ARCH" in
        x86_64|amd64)   ARCH="x86_64" ;;
        aarch64|arm64)  ARCH="aarch64" ;;
        armv7l)         ARCH="arm" ;;
        i686|i386)      ARCH="i686" ;;
        *)              error "Unsupported architecture: $ARCH" ;;
    esac
}

# Get the latest release version
get_latest_version() {
    info "Fetching latest version..."
    
    # Try using curl
    if command -v curl >/dev/null 2>&1; then
        VERSION=$(curl -sSf "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"v?([^"]+)".*/\1/')
    # Try using wget
    elif command -v wget >/dev/null 2>&1; then
        VERSION=$(wget -qO- "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"v?([^"]+)".*/\1/')
    else
        error "Neither curl nor wget is available. Please install one of them."
    fi
    
    if [ -z "$VERSION" ]; then
        error "Failed to fetch latest version"
    fi
    
    info "Latest version: v$VERSION"
}

# Determine target triple
get_target() {
    if [ "$OS" = "linux" ]; then
        if [ "$ARCH" = "x86_64" ]; then
            TARGET="x86_64-unknown-linux-musl"
        elif [ "$ARCH" = "aarch64" ]; then
            TARGET="aarch64-unknown-linux-musl"
        elif [ "$ARCH" = "arm" ]; then
            TARGET="arm-unknown-linux-musleabi"
        elif [ "$ARCH" = "i686" ]; then
            TARGET="i686-unknown-linux-musl"
        else
            error "Unsupported Linux architecture: $ARCH"
        fi
    elif [ "$OS" = "darwin" ]; then
        if [ "$ARCH" = "x86_64" ]; then
            TARGET="x86_64-apple-darwin"
        elif [ "$ARCH" = "aarch64" ]; then
            TARGET="aarch64-apple-darwin"
        else
            error "Unsupported macOS architecture: $ARCH"
        fi
    elif [ "$OS" = "windows" ]; then
        if [ "$ARCH" = "x86_64" ]; then
            TARGET="x86_64-pc-windows-msvc"
        elif [ "$ARCH" = "i686" ]; then
            TARGET="i686-pc-windows-msvc"
        else
            error "Unsupported Windows architecture: $ARCH"
        fi
    else
        error "Unsupported OS: $OS"
    fi
    
    info "Target platform: $TARGET"
}

# Download and extract
download_and_install() {
    # Construct download URL
    if [ "$OS" = "windows" ]; then
        ARCHIVE_NAME="tree-v${VERSION}-${TARGET}.zip"
        ARCHIVE_EXT="zip"
    else
        ARCHIVE_NAME="tree-v${VERSION}-${TARGET}.tar.gz"
        ARCHIVE_EXT="tar.gz"
    fi
    
    DOWNLOAD_URL="https://github.com/$REPO/releases/download/v${VERSION}/${ARCHIVE_NAME}"
    
    info "Downloading from: $DOWNLOAD_URL"
    
    # Create temporary directory
    TMP_DIR=$(mktemp -d)
    cd "$TMP_DIR"
    
    # Download
    if command -v curl >/dev/null 2>&1; then
        curl -sSfL "$DOWNLOAD_URL" -o "$ARCHIVE_NAME" || error "Download failed"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$DOWNLOAD_URL" -O "$ARCHIVE_NAME" || error "Download failed"
    fi
    
    # Extract
    info "Extracting archive..."
    if [ "$ARCHIVE_EXT" = "tar.gz" ]; then
        tar -xzf "$ARCHIVE_NAME" || error "Extraction failed"
    elif [ "$ARCHIVE_EXT" = "zip" ]; then
        unzip -q "$ARCHIVE_NAME" || error "Extraction failed"
    fi
    
    # Find the binary (it might be in a subdirectory)
    if [ "$OS" = "windows" ]; then
        BINARY_PATH=$(find . -name "${BINARY_NAME}.exe" | head -n 1)
    else
        BINARY_PATH=$(find . -name "$BINARY_NAME" -type f | head -n 1)
    fi
    
    if [ -z "$BINARY_PATH" ]; then
        error "Binary not found in archive"
    fi
    
    # Determine installation directory
    if [ -n "$TREE_INSTALL" ]; then
        INSTALL_DIR="$TREE_INSTALL"
    elif [ -w "/usr/local/bin" ]; then
        INSTALL_DIR="/usr/local/bin"
    elif [ -w "$HOME/.local/bin" ]; then
        INSTALL_DIR="$HOME/.local/bin"
        mkdir -p "$INSTALL_DIR"
    else
        INSTALL_DIR="$HOME/.local/bin"
        mkdir -p "$INSTALL_DIR"
    fi
    
    # Install binary
    info "Installing to $INSTALL_DIR..."
    
    if [ -w "$INSTALL_DIR" ]; then
        cp "$BINARY_PATH" "$INSTALL_DIR/" || error "Installation failed"
        chmod +x "$INSTALL_DIR/$BINARY_NAME" || true
    else
        # Try with sudo
        warn "Installing with sudo (requires administrator privileges)..."
        sudo cp "$BINARY_PATH" "$INSTALL_DIR/" || error "Installation failed"
        sudo chmod +x "$INSTALL_DIR/$BINARY_NAME" || true
    fi
    
    # Clean up
    cd - > /dev/null
    rm -rf "$TMP_DIR"
    
    info "${GREEN}✓${NC} tree v$VERSION installed successfully!"
    
    # Check if install directory is in PATH
    case ":$PATH:" in
        *:$INSTALL_DIR:*)
            ;;
        *)
            warn "⚠️  $INSTALL_DIR is not in your PATH"
            warn "   Add the following to your shell config (~/.bashrc, ~/.zshrc, etc.):"
            echo ""
            echo "       export PATH=\"$INSTALL_DIR:\$PATH\""
            echo ""
            ;;
    esac
    
    # Show version
    if command -v "$BINARY_NAME" >/dev/null 2>&1; then
        info "Version check:"
        "$BINARY_NAME" --version || true
    fi
}

# Main execution
main() {
    info "tree installer"
    echo ""
    
    # Check for required tools
    if ! command -v tar >/dev/null 2>&1 && ! command -v unzip >/dev/null 2>&1; then
        error "Neither tar nor unzip is available. Please install one of them."
    fi
    
    detect_os
    detect_arch
    get_latest_version
    get_target
    download_and_install
    
    echo ""
    info "Installation complete! Try running: ${GREEN}tree${NC}"
}

# Allow version to be specified via environment variable
if [ -n "$TREE_VERSION" ]; then
    VERSION="$TREE_VERSION"
fi

main
