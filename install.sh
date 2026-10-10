#!/usr/bin/env sh
# Install the Oscovery for the current user (no Flatpak).
#   sh install.sh            -> ~/.local (binary in ~/.local/bin)
#   PREFIX=/usr sh install.sh -> system-wide (run with sudo)
#   DESTDIR=<dir> PREFIX=/usr sh install.sh -> stage a package (the PKGBUILD):
#     files go under <dir>, the paths written into them don't; the release
#     build must exist already, and caches are left to the package manager.
set -eu
PREFIX="${PREFIX:-$HOME/.local}"
DESTDIR="${DESTDIR:-}"
APP_ID=io.github.turbinebmw.Oscovery
# Where the binary is once installed (written into the desktop file and the
# D-Bus and systemd units), and where files go now.
BIN="$PREFIX/bin"
SHARE="$DESTDIR$PREFIX/share"

if [ -n "$DESTDIR" ]; then
  [ -x target/release/oscovery ] || { echo "Build first: cargo build --release" >&2; exit 1; }
else
  # Check build dependencies up front so a missing one is named, with the package
  # to install, instead of surfacing as a panic halfway through `cargo build`.
  if command -v pacman >/dev/null 2>&1; then distro=arch
  elif command -v apt-get >/dev/null 2>&1; then distro=debian
  elif command -v dnf >/dev/null 2>&1; then distro=fedora
  else distro=unknown; fi

  missing=""
  what=""
  # need <what> <arch pkg> <debian pkg> <fedora pkg>
  need() {
    case $distro in
      arch) pkg=$2 ;; debian) pkg=$3 ;; fedora) pkg=$4 ;; *) pkg=$1 ;;
    esac
    what="$what
    - $1"
    case " $missing " in *" $pkg "*) ;; *) missing="$missing $pkg" ;; esac
  }
  have_cmd() { command -v "$1" >/dev/null 2>&1; }
  # have_lib <pkg-config module> [minimum version]
  have_lib() { pkg-config --exists "$1${2:+ >= $2}" 2>/dev/null; }

  have_cmd cargo || need cargo rust cargo cargo
  have_cmd blueprint-compiler || need blueprint-compiler blueprint-compiler blueprint-compiler blueprint-compiler
  have_cmd glib-compile-schemas || need glib-compile-schemas glib2 libglib2.0-dev-bin glib2-devel
  if have_cmd pkg-config; then
    have_lib gtk4 4.18 || need "gtk4 >= 4.18" gtk4 libgtk-4-dev gtk4-devel
    have_lib libadwaita-1 1.8 || need "libadwaita >= 1.8" libadwaita libadwaita-1-dev libadwaita-devel
    else
    need pkg-config pkgconf pkg-config pkgconf-pkg-config
  fi

  if [ -n "$missing" ]; then
    echo "Oscovery can't be built; missing:$what" >&2
    echo "Install them with:" >&2
    case $distro in
      arch) echo "  sudo pacman -S --needed$missing" >&2 ;;
      debian) echo "  sudo apt install$missing" >&2 ;;
      fedora) echo "  sudo dnf install$missing" >&2 ;;
      *) echo " $missing" >&2 ;;
    esac
    exit 1
  fi

  cargo build --release
fi
install -Dm755 target/release/oscovery "$DESTDIR$BIN/oscovery"
install -Dm644 data/$APP_ID.gschema.xml "$SHARE/glib-2.0/schemas/$APP_ID.gschema.xml"
[ -n "$DESTDIR" ] || glib-compile-schemas "$SHARE/glib-2.0/schemas"
sed "s|^Exec=oscovery|Exec=$BIN/oscovery|" data/$APP_ID.desktop.in > "$SHARE/applications/$APP_ID.desktop.tmp" 2>/dev/null \
  || { install -d "$SHARE/applications"; sed "s|^Exec=oscovery|Exec=$BIN/oscovery|" data/$APP_ID.desktop.in > "$SHARE/applications/$APP_ID.desktop.tmp"; }
mv "$SHARE/applications/$APP_ID.desktop.tmp" "$SHARE/applications/$APP_ID.desktop"
install -d "$SHARE/dbus-1/services"
sed "s|@bindir@|$BIN|" data/$APP_ID.service.in > "$SHARE/dbus-1/services/$APP_ID.service"
# dbus-broker only notices the services directory if it existed when the bus
# started, so a first install stays un-activatable (desktop entry and mailto:
# do nothing) until the next login unless the bus is told to re-read it.
[ -z "$DESTDIR" ] && [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ] && dbus-send --session --dest=org.freedesktop.DBus \
  --type=method_call / org.freedesktop.DBus.ReloadConfig >/dev/null 2>&1 || true
install -Dm644 data/$APP_ID.metainfo.xml.in "$SHARE/metainfo/$APP_ID.metainfo.xml"
for size in 48 64 128 256 512; do
  install -Dm644 "data/icons/hicolor/${size}x${size}/apps/$APP_ID.png" "$SHARE/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
done
install -Dm644 "data/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg" "$SHARE/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg"
if [ -z "$DESTDIR" ]; then
  gtk-update-icon-cache -q -t -f "$SHARE/icons/hicolor" 2>/dev/null || true
  update-desktop-database -q "$SHARE/applications" 2>/dev/null || true
fi
# Translations, when there are any: po/<lang>.po -> share/locale/<lang>/.
if command -v msgfmt >/dev/null 2>&1; then
  for po in po/*.po; do
    [ -e "$po" ] || continue
    lang=$(basename "$po" .po)
    install -d "$SHARE/locale/$lang/LC_MESSAGES"
    msgfmt -o "$SHARE/locale/$lang/LC_MESSAGES/oscovery.mo" "$po"
  done
fi

echo "Installed to $DESTDIR$PREFIX. Run: $BIN/oscovery"
