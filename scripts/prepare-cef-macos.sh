#!/usr/bin/env bash
# Source this file so Cargo inherits the pinned CEF build tools.
set -euo pipefail
cef_tools="$HOME/Library/Caches/muniment-cef-build-tools"
mkdir -p "$cef_tools"
if [ ! -x "$cef_tools/cmake-4.4.3-macos-universal/CMake.app/Contents/bin/cmake" ]; then
  curl -fL --retry 3 https://github.com/Kitware/CMake/releases/download/v4.4.3/cmake-4.4.3-macos-universal.tar.gz -o "$cef_tools/cmake.tar.gz"
  echo "0c5d65251c14cc884bfa16bdbed3c263ce5bffe2e21c0d0d00962cb0610464fa  $cef_tools/cmake.tar.gz" | shasum -a 256 -c -
  tar -xzf "$cef_tools/cmake.tar.gz" -C "$cef_tools"
  rm "$cef_tools/cmake.tar.gz"
fi
if [ ! -x "$cef_tools/ninja-1.13.2/ninja" ]; then
  curl -fL --retry 3 https://github.com/ninja-build/ninja/releases/download/v1.13.2/ninja-mac.zip -o "$cef_tools/ninja.zip"
  echo "c99048673aa765960a99cf10c6ddb9f1fad506099ff0a0e137ad8960a88f321b  $cef_tools/ninja.zip" | shasum -a 256 -c -
  mkdir -p "$cef_tools/ninja-1.13.2"
  unzip -o "$cef_tools/ninja.zip" -d "$cef_tools/ninja-1.13.2"
  rm "$cef_tools/ninja.zip"
fi
export PATH="$cef_tools/cmake-4.4.3-macos-universal/CMake.app/Contents/bin:$cef_tools/ninja-1.13.2:$PATH"
