#!/usr/bin/env bash
# Install, update or remove Lumen, a command launcher for GNOME.
#
#   Install or update:  curl -fsSL https://farvisun.github.io/gnome-launcher/install.sh | bash
#   Remove:             curl -fsSL https://farvisun.github.io/gnome-launcher/install.sh | bash -s -- --uninstall
#
# Lumen is installed from the signed farvisun APT repository, so later updates
# arrive with your normal system updates. You will be asked for your password.
set -euo pipefail

UUID="lumen@gnome-launcher.local"
PACKAGE="lumen-launcher"
APT_SETUP="https://farvisun.github.io/apt/setup.sh"

bold=$'\033[1m'; green=$'\033[32m'; yellow=$'\033[33m'; red=$'\033[31m'; reset=$'\033[0m'
[ -t 1 ] || { bold=; green=; yellow=; red=; reset=; }
step() { printf '%s==>%s %s\n' "$bold" "$reset" "$*"; }
note() { printf '%s!%s %s\n' "$yellow" "$reset" "$*"; }
die() { printf '%serror:%s %s\n' "$red" "$reset" "$*" >&2; exit 1; }

[ "$(id -u)" -ne 0 ] || die "Run this as your normal user, not with sudo. It asks for your password when needed."
for tool in curl sudo apt-get gsettings python3; do
    command -v "$tool" >/dev/null 2>&1 || die "'$tool' is required but not installed."
done

edit_list() { # key add|remove uuid
    local current updated
    current=$(gsettings get org.gnome.shell "$1")
    updated=$(python3 -c '
import ast, sys
raw, action, uuid = sys.argv[1:4]
items = [] if raw.startswith("@as") else ast.literal_eval(raw)
items = [i for i in items if i != uuid]
if action == "add":
    items.append(uuid)
sys.stdout.write(repr(items))
' "$current" "$2" "$3")
    [ "$updated" = "$current" ] || gsettings set org.gnome.shell "$1" "$updated"
}

if [ "${1:-}" = "--uninstall" ]; then
    step "Turning off Lumen's shell integration (this restores your keyboard-layout shortcut)"
    edit_list enabled-extensions remove "$UUID"
    sleep 1
    step "Removing $PACKAGE"
    sudo apt-get remove -y "$PACKAGE"
    printf '%s✓%s Lumen was removed. Your preferences stay in ~/.local/share/gnome-launcher.\n' "$green" "$reset"
    echo "  To also remove the farvisun repository: curl -fsSL $APT_SETUP | sudo bash -s -- --remove"
    exit 0
fi

if [ -x "$HOME/.local/bin/gnome-launcher" ]; then
    die "Lumen is already installed from source in your home folder. Remove that copy first with 'python3 scripts/uninstall.py' in its source folder, then run this again."
fi

shell_version=$(gnome-shell --version 2>/dev/null | sed -n 's/[^0-9]*\([0-9]*\).*/\1/p')
if [ -n "$shell_version" ] && [ "$shell_version" != "50" ]; then
    die "Lumen needs GNOME Shell 50; this computer has GNOME Shell $shell_version."
fi
[ "$(dpkg --print-architecture)" = amd64 ] || die "Lumen packages are available for amd64 (x86-64) only."

step "Adding the farvisun APT repository"
curl -fsSL "$APT_SETUP" | sudo bash -s -- --quiet

was_installed=0
dpkg -s "$PACKAGE" >/dev/null 2>&1 && was_installed=1

step "Installing Lumen"
sudo apt-get install -y "$PACKAGE"

step "Turning on Lumen's shell integration"
edit_list disabled-extensions remove "$UUID"
edit_list enabled-extensions add "$UUID"

echo
if [ "$was_installed" = 1 ]; then
    printf '%s✓%s Lumen is updated. Log out and back in to load the new version.\n' "$green" "$reset"
else
    printf '%s✓%s Lumen is installed.\n' "$green" "$reset"
    echo "  Log out and back in once, then press Super + Space."
    note "Keyboard-layout switching moves to Super + Shift + Space while Lumen is on."
fi
echo "  Updates now come with your normal system updates."
