#!/bin/bash

set -e

if [ "$EUID" -ne 0 ]; then
    echo "Please run this installer with sudo."
    exit 1
fi

echo "Installing PC GamePak..."

# Check for important files

for FILE in \
    "linux/gamepak-launcher-helper.sh" \
    "linux/gamepak-remove-helper.sh" \
    "linux/pc-gamepak@.service" \
    "linux/pc-gamepak-remove@.service" \
    "linux/99-pc-gamepak.rules"
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

install -m 755 linux/gamepak-launcher-helper.sh /usr/local/bin/pc-gamepak-helper
install -m 755 linux/eject.sh /usr/local/bin/pc-gamepak-eject


########################################
# Install systemd template
########################################

echo "Installing systemd service..."

sed "s/__USERNAME__/$USERNAME/g" \
    "linux/pc-gamepak@.service" \
    > /etc/systemd/system/pc-gamepak@.service

sed "s/__USERNAME__/$USERNAME/g" \
    "linux/pc-gamepak-remove@.service" \
    > /etc/systemd/system/pc-gamepak-remove@.service


########################################
# Install removal helper
########################################

echo "Installing removal helper..."

install -m 755 linux/gamepak-remove-helper.sh /usr/local/bin/pc-gamepak-remove


########################################
# Install udev rule
########################################

echo "Installing udev rule..."

install -m 644 linux/99-pc-gamepak.rules /etc/udev/rules.d/99-pc-gamepak.rules


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

LAUNCHER_BUILD="tauri-ui/src-tauri/target/release/pc-gamepak"

if [ -f "$LAUNCHER_BUILD" ]; then
    echo "Installing launcher..."
    install -m 755 "$LAUNCHER_BUILD" /usr/local/bin/pc-gamepak
    LAUNCHER_STATE="installed"
else
    LAUNCHER_STATE="not built yet"
fi


########################################
# Done
########################################

echo ""
echo "=========================================="
echo " PC GamePak installed"
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