#!/bin/sh
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 022

fail() { printf '%s\n' "$1" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail 'Run the AppImage sandbox setup with sudo.'
source_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
helper="$source_dir/usr/lib/muniment/cef/chrome-sandbox"
destination=/usr/lib/muniment/cef/chrome-sandbox
[ -f "$helper" ] && [ -s "$helper" ] && [ ! -L "$helper" ] || fail 'The AppImage lacks a regular Chromium sandbox helper.'

# Keep the privileged helper outside the AppImage FUSE mount.
for directory in /usr /usr/lib /usr/lib/muniment /usr/lib/muniment/cef; do
  if [ ! -e "$directory" ] && [ ! -L "$directory" ]; then
    mkdir -- "$directory"
  fi
  [ -d "$directory" ] && [ ! -L "$directory" ] || fail 'The sandbox directory must not be a symbolic link.'
  [ "$(stat -c %u -- "$directory")" = 0 ] || fail 'Root must own the sandbox directory.'
  mode=$(stat -c %a -- "$directory")
  [ "$((0$mode & 0022))" = 0 ] || fail 'Only root may write to the sandbox directory.'
done
options=$(findmnt --noheadings --output OPTIONS --target /usr/lib/muniment/cef)
case ",$options," in
  ,,|*,nosuid,*|*,noexec,*) fail 'The sandbox helper requires a setuid-enabled executable filesystem.' ;;
esac

# Serialize setup and publish only a complete, checked helper.
lock=/usr/lib/muniment/cef/.sandbox-setup.lock
mkdir -- "$lock" || fail 'Another sandbox setup holds the lock.'
temporary=
trap 'if [ -n "$temporary" ]; then rm -f -- "$temporary"; fi; rmdir -- "$lock"' EXIT
trap 'exit 1' HUP INT TERM
if [ -e "$destination" ] || [ -L "$destination" ]; then
  [ -f "$destination" ] && [ ! -L "$destination" ] &&
    [ "$(stat -c %u:%g:%a -- "$destination")" = 0:0:4755 ] &&
    cmp --silent -- "$helper" "$destination" || fail 'A different sandbox helper exists. Use the DEB or remove the AppImage helper before setup.'
else
  temporary=$(mktemp /usr/lib/muniment/cef/.chrome-sandbox.XXXXXX)
  install -o root -g root -m 4755 -- "$helper" "$temporary"
  [ "$(stat -c %u:%g:%a -- "$temporary")" = 0:0:4755 ] || fail 'The sandbox helper has invalid ownership or permissions.'
  cmp --silent -- "$helper" "$temporary"
  mv -T -- "$temporary" "$destination"
  temporary=
fi
printf '%s\n' 'The AppImage sandbox helper is ready.'
