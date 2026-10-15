#!/bin/bash
# hide's deb and rpm after-install script (PRD auto-update D-13), from
# electron-builder's own template with one change: the terminal's `hide` is
# the CLI in resources/, not the app's window, so `/usr/bin/hide` leads there.
# The window opens from the app menu entry, which names /opt/hide/hide.

cli='/opt/${sanitizedProductName}/resources/${executable}'
if type update-alternatives >/dev/null 2>&1; then
    # Remove a previous link that does not use update-alternatives.
    if [ -L '/usr/bin/${executable}' -a -e '/usr/bin/${executable}' -a "`readlink '/usr/bin/${executable}'`" != '/etc/alternatives/${executable}' ]; then
        rm -f '/usr/bin/${executable}'
    fi
    update-alternatives --install '/usr/bin/${executable}' '${executable}' "$cli" 100 || ln -sf "$cli" '/usr/bin/${executable}'
else
    ln -sf "$cli" '/usr/bin/${executable}'
fi

# The SUID sandbox helper is used only where user namespaces are unavailable.
if ! { [[ -L /proc/self/ns/user ]] && unshare --user true; }; then
    chmod 4755 '/opt/${sanitizedProductName}/chrome-sandbox' || true
else
    chmod 0755 '/opt/${sanitizedProductName}/chrome-sandbox' || true
fi

if hash update-mime-database 2>/dev/null; then
    update-mime-database /usr/share/mime || true
fi

if hash update-desktop-database 2>/dev/null; then
    update-desktop-database /usr/share/applications || true
fi

# Ubuntu 24.04 and later restrict unprivileged user namespaces unless an
# AppArmor profile allows them; the profile is installed only where this
# AppArmor understands it (22.04 runs the app without one).
if apparmor_status --enabled > /dev/null 2>&1; then
  APPARMOR_PROFILE_SOURCE='/opt/${sanitizedProductName}/resources/apparmor-profile'
  APPARMOR_PROFILE_TARGET='/etc/apparmor.d/${executable}'
  if apparmor_parser --skip-kernel-load --debug "$APPARMOR_PROFILE_SOURCE" > /dev/null 2>&1; then
    cp -f "$APPARMOR_PROFILE_SOURCE" "$APPARMOR_PROFILE_TARGET"
    if ! { [ -x '/usr/bin/ischroot' ] && /usr/bin/ischroot; } && hash apparmor_parser 2>/dev/null; then
      apparmor_parser --replace --write-cache --skip-read-cache "$APPARMOR_PROFILE_TARGET"
    fi
  else
    echo "Skipping the installation of the AppArmor profile as this version of AppArmor does not seem to support the bundled profile"
  fi
fi
