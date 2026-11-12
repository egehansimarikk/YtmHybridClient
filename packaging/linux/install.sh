#!/bin/bash
# YTM Hybrid Linux Installation Script
# Installs ytm-hybrid binary, icon, and desktop entry for the current user

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_NAME="ytm-hybrid"
VERSION="0.3.0"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Installation paths
BIN_DIR="$HOME/.local/bin"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
DESKTOP_DIR="$HOME/.local/share/applications"

# Source files (relative to script directory or current directory)
if [ -f "$SCRIPT_DIR/$APP_NAME" ]; then
    BINARY_SRC="$SCRIPT_DIR/$APP_NAME"
elif [ -f "./$APP_NAME" ]; then
    BINARY_SRC="./$APP_NAME"
else
    echo -e "${RED}Error: $APP_NAME binary not found${NC}"
    echo "Expected location: $SCRIPT_DIR/$APP_NAME or ./$APP_NAME"
    exit 1
fi

if [ -f "$SCRIPT_DIR/$APP_NAME.png" ]; then
    ICON_SRC="$SCRIPT_DIR/$APP_NAME.png"
elif [ -f "./$APP_NAME.png" ]; then
    ICON_SRC="./$APP_NAME.png"
else
    echo -e "${RED}Error: $APP_NAME.png icon not found${NC}"
    echo "Expected location: $SCRIPT_DIR/$APP_NAME.png or ./$APP_NAME.png"
    exit 1
fi

# Function to show usage
show_usage() {
    cat << EOF
YTM Hybrid Installation Script v${VERSION}

Usage: $0 [OPTIONS]

OPTIONS:
    --install       Install YTM Hybrid (default)
    --uninstall     Uninstall YTM Hybrid
    -h, --help      Show this help message

INSTALLATION PATHS:
    Binary:   $BIN_DIR/$APP_NAME
    Icon:     $ICON_DIR/$APP_NAME.png
    Desktop:  $DESKTOP_DIR/$APP_NAME.desktop

EOF
}

# Function to install
install_ytm() {
    echo -e "${GREEN}Installing YTM Hybrid v${VERSION}...${NC}"
    echo ""

    # Create directories
    echo "Creating installation directories..."
    mkdir -p "$BIN_DIR"
    mkdir -p "$ICON_DIR"
    mkdir -p "$DESKTOP_DIR"

    # Install binary
    echo "Installing binary to $BIN_DIR..."
    cp "$BINARY_SRC" "$BIN_DIR/$APP_NAME"
    chmod +x "$BIN_DIR/$APP_NAME"

    # Install icon
    echo "Installing icon to $ICON_DIR..."
    cp "$ICON_SRC" "$ICON_DIR/$APP_NAME.png"

    # Create desktop entry
    echo "Creating desktop entry..."
    cat > "$DESKTOP_DIR/$APP_NAME.desktop" << 'DESKTOP_EOF'
[Desktop Entry]
Type=Application
Name=YTM Hybrid
GenericName=Music Player
Comment=YouTube Music streaming client, built with Rust and GPUI
Icon=ytm-hybrid
Exec=ytm-hybrid %U
Terminal=false
Categories=AudioVideo;Audio;Player;
Keywords=music;youtube;audio;player;youtube music;
MimeType=x-scheme-handler/youtube;audio/mpeg;audio/flac;audio/mp4;audio/aac;audio/ogg;audio/opus;audio/x-wav;audio/x-aiff;audio/webm;audio/x-matroska;audio/x-wavpack;audio/x-ape;
StartupWMClass=ytm-hybrid
StartupNotify=true
DESKTOP_EOF

    # Update desktop database
    if command -v update-desktop-database &> /dev/null; then
        echo "Updating desktop database..."
        update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
    fi

    # Update icon cache
    if command -v gtk-update-icon-cache &> /dev/null; then
        echo "Updating icon cache..."
        gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
    fi

    echo ""
    echo -e "${GREEN}✓ Installation complete!${NC}"
    echo ""
    echo "YTM Hybrid has been installed to:"
    echo "  • Binary:  $BIN_DIR/$APP_NAME"
    echo "  • Icon:    $ICON_DIR/$APP_NAME.png"
    echo "  • Desktop: $DESKTOP_DIR/$APP_NAME.desktop"
    echo ""

    # Check if ~/.local/bin is in PATH
    if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
        echo -e "${YELLOW}⚠ Warning: $HOME/.local/bin is not in your PATH${NC}"
        echo "Add this line to your ~/.bashrc or ~/.zshrc:"
        echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        echo ""
    fi

    echo "You can now launch YTM Hybrid from:"
    echo "  • Application menu (search for 'YTM Hybrid')"
    echo "  • Terminal: ytm-hybrid"
    echo ""
}

# Function to uninstall
uninstall_ytm() {
    echo -e "${YELLOW}Uninstalling YTM Hybrid...${NC}"
    echo ""

    # Remove binary
    if [ -f "$BIN_DIR/$APP_NAME" ]; then
        echo "Removing binary..."
        rm -f "$BIN_DIR/$APP_NAME"
    fi

    # Remove icon
    if [ -f "$ICON_DIR/$APP_NAME.png" ]; then
        echo "Removing icon..."
        rm -f "$ICON_DIR/$APP_NAME.png"
    fi

    # Remove desktop entry
    if [ -f "$DESKTOP_DIR/$APP_NAME.desktop" ]; then
        echo "Removing desktop entry..."
        rm -f "$DESKTOP_DIR/$APP_NAME.desktop"
    fi

    # Update desktop database
    if command -v update-desktop-database &> /dev/null; then
        echo "Updating desktop database..."
        update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
    fi

    # Update icon cache
    if command -v gtk-update-icon-cache &> /dev/null; then
        echo "Updating icon cache..."
        gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
    fi

    echo ""
    echo -e "${GREEN}✓ YTM Hybrid has been uninstalled${NC}"
    echo ""
    echo "Note: User data and cache remain in:"
    echo "  • Config:  ~/.config/ytm-hybrid/"
    echo "  • Cache:   ~/.cache/ytm-hybrid/"
    echo "  • Data:    ~/.local/share/ytm-hybrid/"
    echo ""
    echo "Remove these manually if desired:"
    echo "  rm -rf ~/.config/ytm-hybrid ~/.cache/ytm-hybrid ~/.local/share/ytm-hybrid"
    echo ""
}

# Main script logic
case "${1:-}" in
    --uninstall)
        uninstall_ytm
        ;;
    --install|"")
        install_ytm
        ;;
    -h|--help)
        show_usage
        exit 0
        ;;
    *)
        echo -e "${RED}Error: Unknown option '$1'${NC}"
        echo ""
        show_usage
        exit 1
        ;;
esac
