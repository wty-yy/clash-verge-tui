#!/bin/sh
set -eu

target="${TARGET:?TARGET is required}"
architecture="${ARCHITECTURE:?ARCHITECTURE is required}"
version="${APP_VERSION:?APP_VERSION is required}"
core_version="1.19.29"

case "$architecture" in
    x86_64)
        core_asset="mihomo-linux-amd64-v${core_version}.gz"
        core_sha256="60de76a35a6cbf7b4fa4a20f5c257c24345d1d635ab1aa3877022a1997ef413c"
        core_binary_sha256="9c397be7489538628fae781bc005e4c5b8cd7b0961b8bb2ca815c8150f193577"
        ;;
    aarch64)
        core_asset="mihomo-linux-arm64-v${core_version}.gz"
        core_sha256="9a868b5e4e0ad91d9d71e1b41b0cfce78aaba44360c30df74a723f8e3926a86c"
        core_binary_sha256="8e02308f672e89c076bfc2fa1b03379bd54e58b0bafa81ffb01113fcf6da348d"
        ;;
    *)
        printf 'Unsupported release architecture: %s\n' "$architecture" >&2
        exit 1
        ;;
esac

app="target/$target/release/clash-verge-tui"
[ -x "$app" ] || {
    printf 'Missing release binary: %s\n' "$app" >&2
    exit 1
}

asset="clash-verge-tui-v${version}-linux-${architecture}.tar.gz"
mkdir -p dist
staging="$(mktemp -d "dist/package-${architecture}.XXXXXX")"
mkdir -p "$staging/bin" "$staging/lib/clash-verge-tui" \
    "$staging/share/licenses/clash-verge-tui" "$staging/share/licenses/mihomo"
install -m 0755 "$app" "$staging/bin/clash-verge-tui"
install -m 0644 LICENSE "$staging/share/licenses/clash-verge-tui/LICENSE"
install -m 0644 docs/LICENSE-GPL-3.0 "$staging/share/licenses/mihomo/LICENSE"

core_archive="dist/$core_asset"
curl -fL --retry 3 -o "$core_archive" "https://github.com/MetaCubeX/mihomo/releases/download/v${core_version}/${core_asset}"
printf '%s  %s\n' "$core_sha256" "$core_archive" | sha256sum -c -
gzip -dc "$core_archive" > "$staging/lib/clash-verge-tui/mihomo"
chmod 0755 "$staging/lib/clash-verge-tui/mihomo"
printf '%s  %s\n' "$core_binary_sha256" "$staging/lib/clash-verge-tui/mihomo" | sha256sum -c -

cat > "$staging/lib/clash-verge-tui/release.json" <<EOF
{
  "app_version": "$version",
  "mihomo_version": "$core_version",
  "architecture": "$architecture",
  "mihomo_asset": "$core_asset",
  "mihomo_archive_sha256": "$core_sha256",
  "mihomo_binary_sha256": "$core_binary_sha256",
  "mihomo_source": "https://github.com/MetaCubeX/mihomo/tree/v$core_version"
}
EOF

"$staging/bin/clash-verge-tui" --version | grep -F "$version" >/dev/null
"$staging/lib/clash-verge-tui/mihomo" -v | grep -F "v$core_version" >/dev/null
tar -C "$staging" -czf "dist/$asset" .
(cd dist && sha256sum "$asset" > "$asset.sha256")
printf 'Created dist/%s\n' "$asset"
