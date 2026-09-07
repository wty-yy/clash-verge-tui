#!/bin/sh
set -eu
command -v readelf >/dev/null 2>&1 || { echo "readelf is required" >&2; exit 1; }
for binary in "$@"; do
    headers="$(LC_ALL=C readelf -lW "$binary")"
    dynamic="$(LC_ALL=C readelf -dW "$binary")"
    versions="$(LC_ALL=C readelf --version-info "$binary")"
    if printf '%s\n' "$headers" | grep -q 'INTERP' ||
        printf '%s\n' "$dynamic" | grep -q '(NEEDED)' ||
        printf '%s\n' "$versions" | grep -q 'GLIBC_'; then
        printf 'Release binary must be statically linked without glibc: %s\n' "$binary" >&2
        exit 1
    fi
done
