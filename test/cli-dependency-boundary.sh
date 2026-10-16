#!/usr/bin/env bash
set -eu

if [ "$#" -gt 0 ]; then
  packages=$(printf '%s\n' "$@")
else
  manifest=${MUNIMENT_CARGO_MANIFEST:-src-tauri/Cargo.toml}
  packages=$(cargo tree --manifest-path "$manifest" --package muniment-cli \
    --locked --target all --prefix none --format '{p}' | awk '{print $1}')
fi

allowed='muniment-cli|muniment-attach|muniment-code-diff|getrandom|cfg-if|libc|r-efi|wasip2|wit-bindgen|serde|serde_core|serde_derive|serde_json|uuid|itoa|memchr|proc-macro2|quote|syn|unicode-ident|zmij'
# The attach client carries the pinned TLS transport (rustls on ring), the
# remote route URL parser and the SHA-256 certificate fingerprint.
allowed="$allowed|rustls|rustls-pki-types|rustls-webpki|ring|untrusted|zeroize|subtle|once_cell|cc|find-msvc-tools|shlex|wasi"
allowed="$allowed|windows-sys|windows-targets|windows_aarch64_gnullvm|windows_aarch64_msvc|windows_i686_gnu|windows_i686_gnullvm|windows_i686_msvc|windows_x86_64_gnu|windows_x86_64_gnullvm|windows_x86_64_msvc"
allowed="$allowed|sha2|digest|block-buffer|crypto-common|hybrid-array|typenum|const-oid|cpufeatures"
allowed="$allowed|url|form_urlencoded|percent-encoding|idna|idna_adapter|utf8_iter|smallvec|displaydoc|synstructure|litemap|writeable|stable_deref_trait"
allowed="$allowed|icu_collections|icu_locale_core|icu_normalizer|icu_normalizer_data|icu_properties|icu_properties_data|icu_provider|potential_utf|tinystr"
allowed="$allowed|yoke|yoke-derive|zerofrom|zerofrom-derive|zerotrie|zerovec|zerovec-derive"
unexpected=$(printf '%s\n' "$packages" | grep -Ev "^($allowed)$" || true)
if [ -n "$unexpected" ]; then
  printf 'muniment-cli includes dependencies outside the protocol boundary:\n%s\n' \
    "$unexpected" >&2
  exit 1
fi
