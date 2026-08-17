#!/bin/bash

set -e

if [ "$EUID" -ne 0 ]; then
    echo "Please run this installer with sudo."
    exit 1
fi

echo "Installing PC Cartridge System..."

# Check for important files

for FILE in \
    "linux/cartridge-launcher-helper.sh" \
    "linux/cartridge-remove-helper.sh" \
    "linux/game-cartridge@.service" \
    "linux/game-cartridge-remove@.service" \
    "linux/99-game-cartridge.rules"
do
    if [ ! -f "$FILE" ]; then
        echo "Missing file: $FILE"
        exit 1
    fi
done

########################################
# Detect user
########################################

if [ -n "$SUDO_USER" ]; then
    USERNAME="$SUDO_USER"
else
    USERNAME="$USER"
fi

USER_HOME=$(eval echo "~$USERNAME")

echo "Installing for user: $USERNAME"
echo "Home directory: $USER_HOME"


########################################
# Install launcher helper
########################################

echo "Installing launcher helper..."

install -m 755 linux/cartridge-launcher-helper.sh /usr/local/bin/pc-cartridge-system-helper
install -m 755 linux/eject.sh /usr/local/bin/pc-cartridge-eject


########################################
# Install systemd template
########################################

echo "Installing systemd service..."

sed "s/__USERNAME__/$USERNAME/g" \
    "linux/game-cartridge@.service" \
    > /etc/systemd/system/game-cartridge@.service

sed "s/__USERNAME__/$USERNAME/g" \
    "linux/game-cartridge-remove@.service" \
    > /etc/systemd/system/game-cartridge-remove@.service


########################################
# Install removal helper
########################################

echo "Installing removal helper..."

install -m 755 linux/cartridge-remove-helper.sh /usr/local/bin/pc-cartridge-system-remove


########################################
# Install udev rule
########################################

echo "Installing udev rule..."

install -m 644 linux/99-game-cartridge.rules /etc/udev/rules.d/99-game-cartridge.rules


########################################
# Reload services
########################################

systemctl daemon-reload

udevadm control --reload-rules

udevadm trigger


########################################
# Done
########################################

########################################
# Install the launcher binary if it has been built
########################################

LAUNCHER_BUILD="tauri-ui/src-tauri/target/release/pc-cartridge-launcher"

if [ -f "$LAUNCHER_BUILD" ]; then
    echo "Installing launcher..."
    install -m 755 "$LAUNCHER_BUILD" /usr/local/bin/pc-cartridge-launcher
    LAUNCHER_STATE="installed"
else
    LAUNCHER_STATE="not built yet"
fi


########################################
# Done
########################################

echo ""
echo "=========================================="
echo " PC Cartridge System installed"
echo "=========================================="
echo ""
echo " Launcher: $LAUNCHER_STATE"

if [ "$LAUNCHER_STATE" != "installed" ]; then
    echo ""
    echo " Build it, then run this installer again:"
    echo ""
    echo "   cd tauri-ui && npm install && npm run build"
fi

echo ""
echo " Put a cartridge.conf at the root of the drive:"
echo ""
echo "   executable=steam://rungameid/12345"
echo "   title=My Game"
echo "   cover=cover.jpg"
echo ""
echo " Then plug the cartridge in. Nothing runs on its own:"
echo " the launcher opens and waits for you to press Play."
echo ""
echo " The drive must be automounted by your desktop."
echo " If your distro does not automount, install"
echo " something like udiskie."
echo ""