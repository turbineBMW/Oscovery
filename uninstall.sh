#!/usr/bin/env sh
# Remove what install.sh put in place.
#   sh uninstall.sh                  -> from ~/.local
#   sh uninstall.sh --purge          -> and your catalog, cache and settings
#   PREFIX=/usr sh uninstall.sh      -> a system-wide install (run with sudo)
set -eu
PREFIX="${PREFIX:-$HOME/.local}"
APP_ID=io.github.turbinebmw.Oscovery
BIN="$PREFIX/bin"
SHARE="$PREFIX/share"

purge=false
for arg in "$@"; do
  case $arg in
    --purge) purge=true ;;
    -h|--help) sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $arg (try --help)" >&2; exit 2 ;;
  esac
done
is_root=false
if [ "$(id -u)" = 0 ]; then is_root=true; fi
if $purge && $is_root; then
  echo "--purge removes one user's data: run 'sh uninstall.sh --purge' as that user." >&2
  exit 2
fi

# A running Oscovery (it may be in the background) quits the way Quit does.
if ! $is_root && pgrep -x oscovery >/dev/null 2>&1; then
  gapplication action "$APP_ID" quit 2>/dev/null || true
fi

# The D-Bus service goes first, so nothing can start Oscovery again mid-way.
rm -f "$SHARE/dbus-1/services/$APP_ID.service"
rm -f "$BIN/oscovery"
rm -f "$SHARE/applications/$APP_ID.desktop"
rm -f "$SHARE/metainfo/$APP_ID.metainfo.xml"
for size in 48 64 128 256 512; do
  rm -f "$SHARE/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
done
rm -f "$SHARE/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg"
for mo in "$SHARE"/locale/*/LC_MESSAGES/oscovery.mo; do
  if [ -e "$mo" ]; then rm -f "$mo"; fi
done
schemas="$SHARE/glib-2.0/schemas"
if [ -e "$schemas/$APP_ID.gschema.xml" ]; then
  rm -f "$schemas/$APP_ID.gschema.xml"
  # With no schema left to compile, glib-compile-schemas keeps the old
  # compiled file, which still lists Oscovery's.
  if ls "$schemas"/*.gschema.xml >/dev/null 2>&1; then
    glib-compile-schemas "$schemas" >/dev/null 2>&1 || true
  else
    rm -f "$schemas/gschemas.compiled"
  fi
fi
gtk-update-icon-cache -q -t -f "$SHARE/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$SHARE/applications" 2>/dev/null || true

if $purge; then
  rm -rf "${XDG_DATA_HOME:-$HOME/.local/share}/oscovery"
  rm -rf "${XDG_CACHE_HOME:-$HOME/.cache}/oscovery"
  if command -v dconf >/dev/null 2>&1; then
    dconf reset -f /io/github/turbinebmw/Oscovery/ 2>/dev/null || true
  fi
  echo "Removed Oscovery's catalog, cache and settings."
fi

echo "Uninstalled Oscovery from $PREFIX."
