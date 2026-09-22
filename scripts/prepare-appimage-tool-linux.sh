#!/usr/bin/env bash
set -euo pipefail

cache=${1:?Tauri cache directory is required}
destination="$cache/muniment-appimage-tools-4.7.5"
[[ -x "$destination/usr/bin/linuxdeploy-plugin-appimage" ]] && exit 0
work=$(mktemp -d "$cache/muniment-appimage-tools.XXXXXX")
trap 'rm -rf "$work"' EXIT

# The bundled 4.6.1 compressor can produce unreadable files in this large image.
# Pin the replacement source and validate every file after the final packaging.
curl --fail --location --retry 3 --retry-all-errors \
  https://github.com/plougher/squashfs-tools/releases/download/4.7.5/squashfs-tools-4.7.5.tar.gz \
  --output "$work/source.tar.gz"
printf '%s  %s\n' 547b7b7f4d2e44bf91b6fc554664850c69563701deab9fd9cd7e21f694c88ea6 "$work/source.tar.gz" | sha256sum --check
tar -xzf "$work/source.tar.gz" -C "$work"
make -C "$work/squashfs-tools-4.7.5/squashfs-tools" -j2 \
  CONFIG=1 GZIP_SUPPORT=1 ZSTD_SUPPORT=1 XZ_SUPPORT=0 LZO_SUPPORT=0 LZ4_SUPPORT=0 COMP_DEFAULT=gzip \
  XATTR_SUPPORT=1 XATTR_OS_SUPPORT=1 mksquashfs
(
  cd "$work"
  "$cache/linuxdeploy-plugin-appimage.AppImage" --appimage-extract >/dev/null
)
cp "$work/squashfs-tools-4.7.5/squashfs-tools/mksquashfs" \
  "$work/squashfs-root/appimagetool-prefix/usr/bin/mksquashfs"
mv "$work/squashfs-root" "$destination"
