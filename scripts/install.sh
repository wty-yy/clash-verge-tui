#!/bin/sh
set -eu

release_version="v1.4.5"
source="github"
github_proxy="${CLASH_VERGE_TUI_GITHUB_PROXY:-https://gh-proxy.com}"
repository="${CLASH_VERGE_TUI_REPOSITORY:-https://github.com/wty-yy/clash-verge-tui}"
install_dir="${CLASH_VERGE_TUI_INSTALL_DIR:-${HOME}/.local/bin}"
library_dir="$(dirname "$install_dir")/lib/clash-verge-tui"
core_dir="$library_dir/core/v1.19.29"
version="${CLASH_VERGE_TUI_VERSION:-}"

fail() {
    printf 'clash-verge-tui: %s\n' "$1" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --source)
            [ "$#" -ge 2 ] || fail "--source requires github or proxy"
            case "$2" in
                github|proxy) source="$2"; repository="https://github.com/wty-yy/clash-verge-tui" ;;
                *) fail "unsupported source: $2 (use github or proxy)" ;;
            esac
            shift 2
            ;;
        --github-proxy)
            [ "$#" -ge 2 ] || fail "--github-proxy requires an HTTPS prefix"
            github_proxy="$2"
            source="proxy"
            shift 2
            ;;
        -h | --help)
            printf 'Usage: sh install.sh [--source github|proxy] [--github-proxy https://ghfast.top]\n'
            printf 'Optional environment: CLASH_VERGE_TUI_VERSION, CLASH_VERGE_TUI_REPOSITORY, CLASH_VERGE_TUI_INSTALL_DIR, CLASH_VERGE_TUI_ASSET_BASE_URL, CLASH_VERGE_TUI_GITHUB_PROXY\n'
            exit 0
            ;;
        *) fail "unknown argument: $1" ;;
    esac
done
repository="${repository%/}"
repository="${repository%.git}"
github_proxy="${github_proxy%/}"
if [ "$source" = proxy ]; then
    case "$github_proxy" in
        https://?*) ;;
        *) fail "GitHub proxy must be an HTTPS prefix" ;;
    esac
    case "$github_proxy" in
        *[[:space:]]*|*\?*|*\#*|*\@*) fail "invalid GitHub proxy prefix" ;;
    esac
fi

command -v curl >/dev/null 2>&1 || fail "curl is required"
# Older distributions ship curl without --retry-all-errors (added in 7.71.0).
retry_all_errors=
if curl --retry-all-errors --version >/dev/null 2>&1; then
    retry_all_errors=--retry-all-errors
fi

command -v tar >/dev/null 2>&1 || fail "tar is required"
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is required"
[ "$(uname -s)" = "Linux" ] || fail "only Linux is supported"

case "$(uname -m)" in
    x86_64 | amd64) architecture="x86_64" ;;
    aarch64 | arm64) architecture="aarch64" ;;
    *) fail "unsupported architecture: $(uname -m)" ;;
esac

if [ -z "$version" ]; then
    if [ "$source" = proxy ]; then
        # Release scripts pin their own version, avoiding direct GitHub/API discovery.
        version="$release_version"
    else
        latest_url="$(curl -fsSL --connect-timeout 15 --max-time 60 --retry 3 --retry-delay 2 ${retry_all_errors} -o /dev/null -w '%{url_effective}' "$repository/releases/latest")"
        version="${latest_url##*/}"
    fi
fi
printf '%s\n' "$version" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' || fail "invalid release version: $version"
numeric_version="${version#v}"

asset="clash-verge-tui-${version}-linux-${architecture}.tar.gz"
base_url="${CLASH_VERGE_TUI_ASSET_BASE_URL:-$repository/releases/download/$version}"
if [ "$source" = proxy ] && [ -z "${CLASH_VERGE_TUI_ASSET_BASE_URL:-}" ]; then
    base_url="$github_proxy/$base_url"
fi
temporary_dir="$(mktemp -d)"
cleanup() {
    find "$temporary_dir" -type f -delete 2>/dev/null || true
    find "$temporary_dir" -type l -delete 2>/dev/null || true
    find "$temporary_dir" -depth -type d -empty -delete 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM

printf 'Downloading clash-verge-tui %s for Linux %s...\n' "$version" "$architecture"
printf 'Source: %s\n' "$base_url"
curl -fL --connect-timeout 15 --max-time 300 --speed-limit 1024 --speed-time 30 --retry 3 --retry-delay 2 ${retry_all_errors} \
    -o "$temporary_dir/$asset" "$base_url/$asset" || fail "release bundle unavailable at $base_url; try --github-proxy https://ghfast.top or --source github"
curl -fL --connect-timeout 15 --max-time 60 --retry 3 --retry-delay 2 ${retry_all_errors} \
    -o "$temporary_dir/$asset.sha256" "$base_url/$asset.sha256" || fail "release checksum unavailable at $base_url; refusing to install"
(cd "$temporary_dir" && sha256sum -c "$asset.sha256")

mkdir -p "$temporary_dir/package"
tar -xzf "$temporary_dir/$asset" -C "$temporary_dir/package" --no-same-owner
app="$temporary_dir/package/bin/clash-verge-tui"
core="$temporary_dir/package/lib/clash-verge-tui/mihomo"
geosite="$temporary_dir/package/lib/clash-verge-tui/GeoSite.dat"
[ -f "$app" ] && [ ! -L "$app" ] || fail "release does not contain a regular TUI binary"
[ -f "$core" ] && [ ! -L "$core" ] || fail "release does not contain a regular mihomo binary"
[ -f "$geosite" ] && [ -s "$geosite" ] && [ ! -L "$geosite" ] || fail "release does not contain GeoSite.dat"
[ -f "$temporary_dir/package/share/licenses/meta-rules-dat/LICENSE" ] || fail "release does not contain the GeoSite license"
"$app" --version | grep -F "$numeric_version" >/dev/null || fail "TUI version verification failed"
"$core" -v | grep -F 'v1.19.29' >/dev/null || fail "mihomo version verification failed"

mkdir -p "$install_dir" "$core_dir"
install -m 0755 "$app" "$install_dir/clash-verge-tui.new"
install -m 0755 "$core" "$core_dir/mihomo.new"
install -m 0644 "$geosite" "$library_dir/GeoSite.dat.new"
mv -f "$core_dir/mihomo.new" "$core_dir/mihomo"
mv -f "$library_dir/GeoSite.dat.new" "$library_dir/GeoSite.dat"
mv -f "$install_dir/clash-verge-tui.new" "$install_dir/clash-verge-tui"
install -m 0644 "$temporary_dir/package/lib/clash-verge-tui/release.json" "$library_dir/release.json"
install -m 0644 "$temporary_dir/package/share/licenses/clash-verge-tui/LICENSE" "$library_dir/LICENSE"
install -m 0644 "$temporary_dir/package/share/licenses/mihomo/LICENSE" "$library_dir/MIHOMO-LICENSE"
install -m 0644 "$temporary_dir/package/share/licenses/meta-rules-dat/LICENSE" "$library_dir/GEOSITE-LICENSE"

# Older releases predate the musl bundle.
if [ -f "$temporary_dir/package/share/licenses/musl/LICENSE" ]; then
    install -m 0644 "$temporary_dir/package/share/licenses/musl/LICENSE" "$library_dir/MUSL-LICENSE"
fi

printf '\nInstalled clash-verge-tui %s with mihomo v1.19.29.\n' "$version"
printf 'Binary: %s\n' "$install_dir/clash-verge-tui"
printf 'Core: %s\n' "$core_dir/mihomo"
case ":${PATH}:" in
    *":$install_dir:"*) ;;
    *) printf 'Add %s to PATH, then run: clash-verge-tui\n' "$install_dir" ;;
esac
