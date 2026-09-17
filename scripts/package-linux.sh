#!/bin/sh
set -eu

target="${TARGET:?TARGET is required}"
architecture="${ARCHITECTURE:?ARCHITECTURE is required}"
version="${APP_VERSION:?APP_VERSION is required}"
core_version="1.19.29"
geosite_revision="464ce81256c01af2ea0d464e0481fe4726519dcf"
geosite_url="https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/$geosite_revision/geosite.dat"
geosite_sha256="c5fe9448d979391192f5bd553b5e28c39efdc9bd857b7c879a7d995fded0c3fe"
geodata_url="https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/$geosite_revision/geoip.metadb"
geodata_sha256="4eda34a0851c96259fdc2330aeb2173beec58f2534b80da6e3a89e488a18c672"
ui_revision="28a9589f6239bbafc24e87bbf5e5b4997fe42e59"
ui_url="https://codeload.github.com/MetaCubeX/metacubexd/tar.gz/$ui_revision"
ui_sha256="335c0cd44190bf341bf1491e442fdf7e84af6e14e0dbedf613122f9a68e583ab"
ui_license_revision="4aeaa2c545bbe2a7e015debee35290367d292350"
ui_license_sha256="cd0735ba06f26a0008bbca399890c7ca87fe129aacc302c2e33fb03e60a4e8c3"

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

app="${CARGO_TARGET_DIR:-target}/$target/release/clash-verge-tui"
[ -x "$app" ] || {
    printf 'Missing release binary: %s\n' "$app" >&2
    exit 1
}

# A successful build alone does not guarantee a portable static executable.
sh scripts/check-static.sh "$app"

asset="clash-verge-tui-v${version}-linux-${architecture}.tar.gz"
mkdir -p dist
staging="$(mktemp -d "dist/package-${architecture}.XXXXXX")"
mkdir -p "$staging/bin" "$staging/lib/clash-verge-tui" \
    "$staging/share/licenses/clash-verge-tui" "$staging/share/licenses/mihomo" "$staging/share/licenses/musl"
install -m 0755 "$app" "$staging/bin/clash-verge-tui"
install -m 0644 LICENSE "$staging/share/licenses/clash-verge-tui/LICENSE"
install -m 0644 docs/LICENSE-MUSL "$staging/share/licenses/musl/LICENSE"
install -m 0644 docs/LICENSE-GPL-3.0 "$staging/share/licenses/mihomo/LICENSE"
mkdir -p "$staging/share/licenses/meta-rules-dat" "$staging/share/licenses/metacubexd"
install -m 0644 docs/LICENSE-GPL-3.0 "$staging/share/licenses/meta-rules-dat/LICENSE"

core_archive="dist/$core_asset"
curl -fL --connect-timeout 15 --retry 5 --retry-delay 2 --retry-all-errors \
    -o "$core_archive" "https://github.com/MetaCubeX/mihomo/releases/download/v${core_version}/${core_asset}"
printf '%s  %s\n' "$core_sha256" "$core_archive" | sha256sum -c -
gzip -dc "$core_archive" > "$staging/lib/clash-verge-tui/mihomo"
chmod 0755 "$staging/lib/clash-verge-tui/mihomo"
printf '%s  %s\n' "$core_binary_sha256" "$staging/lib/clash-verge-tui/mihomo" | sha256sum -c -

geosite="dist/geosite.dat"
curl -fL --connect-timeout 15 --retry 5 --retry-delay 2 --retry-all-errors \
    -o "$geosite" "$geosite_url"
printf '%s  %s\n' "$geosite_sha256" "$geosite" | sha256sum -c -
install -m 0644 "$geosite" "$staging/lib/clash-verge-tui/GeoSite.dat"

# GeoData mode needs the meta database; without it the core would download it
# from GitHub through a direct connection during its first configuration parse.
geodata="dist/geoip.metadb"
curl -fL --connect-timeout 15 --retry 5 --retry-delay 2 --retry-all-errors \
    -o "$geodata" "$geodata_url"
printf '%s  %s\n' "$geodata_sha256" "$geodata" | sha256sum -c -
install -m 0644 "$geodata" "$staging/lib/clash-verge-tui/geoip.metadb"

ui_archive="dist/metacubexd.tar.gz"
curl -fL --connect-timeout 15 --retry 5 --retry-delay 2 --retry-all-errors \
    -o "$ui_archive" "$ui_url"
printf '%s  %s\n' "$ui_sha256" "$ui_archive" | sha256sum -c -
mkdir -p "$staging/lib/clash-verge-tui/ui"
tar -xzf "$ui_archive" -C "$staging/lib/clash-verge-tui/ui" --strip-components=1 --no-same-owner
[ -f "$staging/lib/clash-verge-tui/ui/index.html" ] || {
    printf 'metacubexd archive does not contain index.html\n' >&2
    exit 1
}
ui_license="dist/metacubexd-LICENSE"
curl -fL --connect-timeout 15 --retry 5 --retry-delay 2 --retry-all-errors \
    -o "$ui_license" "https://raw.githubusercontent.com/MetaCubeX/metacubexd/$ui_license_revision/LICENSE"
printf '%s  %s\n' "$ui_license_sha256" "$ui_license" | sha256sum -c -
install -m 0644 "$ui_license" "$staging/share/licenses/metacubexd/LICENSE"

sh scripts/check-static.sh "$staging/lib/clash-verge-tui/mihomo"

cat > "$staging/lib/clash-verge-tui/release.json" <<EOF
{
  "app_version": "$version",
  "mihomo_version": "$core_version",
  "architecture": "$architecture",
  "target": "$target",
  "linkage": "static",
  "mihomo_asset": "$core_asset",
  "mihomo_archive_sha256": "$core_sha256",
  "mihomo_binary_sha256": "$core_binary_sha256",
  "geosite_file": "GeoSite.dat",
  "geosite_sha256": "$geosite_sha256",
  "geosite_url": "$geosite_url",
  "geosite_source": "https://github.com/MetaCubeX/meta-rules-dat/tree/$geosite_revision",
  "geodata_file": "geoip.metadb",
  "geodata_sha256": "$geodata_sha256",
  "geodata_url": "$geodata_url",
  "geodata_source": "https://github.com/MetaCubeX/meta-rules-dat/tree/$geosite_revision",
  "ui_archive_sha256": "$ui_sha256",
  "ui_url": "$ui_url",
  "ui_source": "https://github.com/MetaCubeX/metacubexd/tree/$ui_revision",
  "mihomo_source": "https://github.com/MetaCubeX/mihomo/tree/v$core_version"
}
EOF

"$staging/bin/clash-verge-tui" --version | grep -F "$version" >/dev/null
"$staging/lib/clash-verge-tui/mihomo" -v | grep -F "v$core_version" >/dev/null
tar -C "$staging" -czf "dist/$asset" .
(cd dist && sha256sum "$asset" > "$asset.sha256")
printf 'Created dist/%s\n' "$asset"
