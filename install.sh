#!/bin/sh
# Installs `muzak`, the tool that sets up Muzak players:
#   curl -fsSL https://raw.githubusercontent.com/mberg/muzak-player/main/install.sh | sh
# Set MUZAK_INSTALL_DIR to install somewhere other than ~/.local/bin.
set -eu

REPO=mberg/muzak-player
case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) name=muzak-macos-arm64 ;;
    Darwin-x86_64) name=muzak-macos-x86_64 ;;
    Linux-x86_64) name=muzak-linux-x86_64 ;;
    Linux-aarch64 | Linux-arm64) name=muzak-linux-arm64 ;;
    *)
        echo "Sorry, there's no muzak for $(uname -s) $(uname -m) yet." >&2
        exit 1
        ;;
esac

dir=${MUZAK_INSTALL_DIR:-$HOME/.local/bin}
mkdir -p "$dir"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "Downloading $name…"
curl -fsSL "https://github.com/$REPO/releases/latest/download/$name.tar.gz" | tar xz -C "$tmp"
install -m 755 "$tmp/muzak" "$dir/muzak"
echo "Installed $("$dir/muzak" --version) to $dir/muzak"

case ":$PATH:" in
    *":$dir:"*) ;;
    *)
        echo
        echo "$dir isn't on your PATH yet. Add this line to ~/.zshrc (or ~/.bashrc), then open a new terminal:"
        echo "  export PATH=\"$dir:\$PATH\""
        ;;
esac
echo
echo "Next: muzak setup"
