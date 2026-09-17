//! Pinned external sources and mirror routes.
//!
//! Runtime configuration and updates prefer the project mirror, so a blocked
//! GitHub does not stall the first start. Release bundles seed the same
//! GeoData and Web UI files offline; the mirror stays a fallback for updates.
use serde_yaml_ng::{Mapping, Value};

pub const MIRROR_ROOT: &str = "https://clash-verge-tui.wty-yy.top";
pub const GITHUB_ROOT: &str = "https://github.com";
pub const GITHUB_API_ROOT: &str = "https://api.github.com";
pub const PROJECT_REPOSITORY: &str = "wty-yy/clash-verge-tui";
pub const MIHOMO_REPOSITORY: &str = "MetaCubeX/mihomo";
pub const META_RULES_REPOSITORY: &str = "MetaCubeX/meta-rules-dat";

/// `geox-url` keys mapped to the file names served under `/geodata`.
const GEOX_FILES: [(&str, &str); 4] = [
    ("mmdb", "geoip.metadb"),
    ("geo-ip", "geoip.dat"),
    ("geo-site", "geosite.dat"),
    ("asn", "GeoLite2-ASN.mmdb"),
];

pub fn geodata_url(file: &str) -> String {
    format!("{MIRROR_ROOT}/geodata/{file}")
}

pub fn core_url(version: &str, asset: &str) -> String {
    format!("{MIRROR_ROOT}/core/v{version}/{asset}")
}

pub fn core_github_url(version: &str, asset: &str) -> String {
    format!("{GITHUB_ROOT}/{MIHOMO_REPOSITORY}/releases/download/v{version}/{asset}")
}

/// Mirror URL for the Web UI archive; mihomo accepts the tar.gz it serves.
pub fn ui_url(selection: &str) -> String {
    let panel = match selection {
        "Yacd" => "yacd-meta",
        _ => "metacubexd",
    };
    format!("{MIRROR_ROOT}/ui/{panel}.tar.gz")
}

/// `geox-url` YAML pointing at the mirror.
pub fn mirror_geox() -> Value {
    Value::Mapping(
        GEOX_FILES
            .iter()
            .map(|(key, file)| (Value::from(*key), Value::from(geodata_url(file))))
            .collect::<Mapping>(),
    )
}

pub fn latest_version_url() -> String {
    format!("{MIRROR_ROOT}/latest-version")
}

pub fn github_latest_url() -> String {
    format!("{GITHUB_ROOT}/{PROJECT_REPOSITORY}/releases/latest")
}

pub fn github_tags_url() -> String {
    format!("{GITHUB_API_ROOT}/repos/{PROJECT_REPOSITORY}/tags?per_page=100")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mirror_route_is_absolute_https() {
        for url in [
            geodata_url("geoip.metadb"),
            ui_url("MetaCubeXD"),
            core_url("1.19.29", "mihomo-linux-amd64-v1.19.29.gz"),
            latest_version_url(),
        ] {
            let url = url::Url::parse(&url).unwrap();
            assert_eq!(url.scheme(), "https");
            assert_eq!(url.host_str(), Some("clash-verge-tui.wty-yy.top"));
        }
    }

    #[test]
    fn mirror_geox_covers_mihomo_keys() {
        let mapping = mirror_geox();
        for key in ["mmdb", "geo-ip", "geo-site", "asn"] {
            assert!(
                mapping[key]
                    .as_str()
                    .is_some_and(|url| url.starts_with(MIRROR_ROOT)),
                "{key}"
            );
        }
        assert_eq!(mapping["mmdb"], geodata_url("geoip.metadb"));
    }
}
