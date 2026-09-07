#!/bin/sh
set -eu

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
            [ "$#" -ge 2 ] || fail "--source requires github or gitee"
            case "$2" in
                github) repository="https://github.com/wty-yy/clash-verge-tui" ;;
                gitee) repository="https://gitee.com/wty-yy/clash-verge-tui" ;;
                *) fail "unsupported source: $2 (use github or gitee)" ;;
            esac
            shift 2
            ;;
        -h | --help)
            printf 'Usage: sh install.sh [--source github|gitee]\n'
            printf 'Optional environment: CLASH_VERGE_TUI_VERSION, CLASH_VERGE_TUI_REPOSITORY, CLASH_VERGE_TUI_INSTALL_DIR, CLASH_VERGE_TUI_ASSET_BASE_URL\n'
            exit 0
            ;;
        *) fail "unknown argument: $1" ;;
    esac
done
repository="${repository%/}"
repository="${repository%.git}"

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is required"
[ "$(uname -s)" = "Linux" ] || fail "only Linux is supported"

case "$(uname -m)" in
    x86_64 | amd64) architecture="x86_64" ;;
    aarch64 | arm64) architecture="aarch64" ;;
    *) fail "unsupported architecture: $(uname -m)" ;;
esac

if [ -z "$version" ]; then
    case "$repository" in
        https://gitee.com/*)
            repository_path="${repository#https://gitee.com/}"
            release="$(curl -fsSL --connect-timeout 15 --retry 5 --retry-delay 2 --retry-all-errors \
                "https://gitee.com/api/v5/repos/$repository_path/releases/latest")" || \
                fail "cannot find a Gitee release; publish the release bundles and checksums at $repository/releases first"
            # Extract only a stable version tag; do not evaluate API response content.
            version="$(printf '%s\n' "$release" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\(v[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)".*/\1/p' | head -1)"
            [ -n "$version" ] || fail "Gitee release has no stable version tag; check $repository/releases"
            ;;
        *)
            latest_url="$(curl -fsSL --connect-timeout 15 --retry 5 --retry-delay 2 --retry-all-errors -o /dev/null -w '%{url_effective}' "$repository/releases/latest")"
            version="${latest_url##*/}"
            ;;
    esac
fi
printf '%s\n' "$version" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' || fail "invalid release version: $version"
numeric_version="${version#v}"

asset="clash-verge-tui-${version}-linux-${architecture}.tar.gz"
base_url="${CLASH_VERGE_TUI_ASSET_BASE_URL:-$repository/releases/download/$version}"
temporary_dir="$(mktemp -d)"
cleanup() {
    find "$temporary_dir" -type f -delete 2>/dev/null || true
    find "$temporary_dir" -type l -delete 2>/dev/null || true
    find "$temporary_dir" -depth -type d -empty -delete 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM

printf 'Downloading clash-verge-tui %s for Linux %s...\n' "$version" "$architecture"
printf 'Source: %s\n' "$base_url"
curl -fL --connect-timeout 15 --retry 5 --retry-delay 2 --retry-all-errors \
    -o "$temporary_dir/$asset" "$base_url/$asset" || fail "release bundle unavailable at $base_url; repository sync alone does not copy release attachments"
curl -fL --connect-timeout 15 --retry 5 --retry-delay 2 --retry-all-errors \
    -o "$temporary_dir/$asset.sha256" "$base_url/$asset.sha256" || fail "release checksum unavailable at $base_url; refusing to install"
(cd "$temporary_dir" && sha256sum -c "$asset.sha256")

mkdir -p "$temporary_dir/package"
tar -xzf "$temporary_dir/$asset" -C "$temporary_dir/package" --no-same-owner
app="$temporary_dir/package/bin/clash-verge-tui"
core="$temporary_dir/package/lib/clash-verge-tui/mihomo"
[ -f "$app" ] && [ ! -L "$app" ] || fail "release does not contain a regular TUI binary"
[ -f "$core" ] && [ ! -L "$core" ] || fail "release does not contain a regular mihomo binary"
"$app" --version | grep -F "$numeric_version" >/dev/null || fail "TUI version verification failed"
"$core" -v | grep -F 'v1.19.29' >/dev/null || fail "mihomo version verification failed"

mkdir -p "$install_dir" "$core_dir"
install -m 0755 "$app" "$install_dir/clash-verge-tui.new"
install -m 0755 "$core" "$core_dir/mihomo.new"
mv -f "$core_dir/mihomo.new" "$core_dir/mihomo"
mv -f "$install_dir/clash-verge-tui.new" "$install_dir/clash-verge-tui"
install -m 0644 "$temporary_dir/package/lib/clash-verge-tui/release.json" "$library_dir/release.json"
install -m 0644 "$temporary_dir/package/share/licenses/clash-verge-tui/LICENSE" "$library_dir/LICENSE"
install -m 0644 "$temporary_dir/package/share/licenses/mihomo/LICENSE" "$library_dir/MIHOMO-LICENSE"

printf '\nInstalled clash-verge-tui %s with mihomo v1.19.29.\n' "$version"
printf 'Binary: %s\n' "$install_dir/clash-verge-tui"
printf 'Core: %s\n' "$core_dir/mihomo"
case ":${PATH}:" in
    *":$install_dir:"*) ;;
    *) printf 'Add %s to PATH, then run: clash-verge-tui\n' "$install_dir" ;;
esac
