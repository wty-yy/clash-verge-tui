#![cfg(target_os = "linux")]
use clash_verge_tui::{network, platform::SystemProxy};
use std::collections::BTreeMap;
#[test]
fn runtime_dns_defaults_match_the_settings_form() {
    let fields = network::default_runtime_fields();
    let settings = clash_verge_tui::settings::defaults();
    for (key, value) in &fields {
        if key != "mode" {
            assert_eq!(settings.get(key), Some(value), "{key}");
        }
    }
    let mut overrides = serde_yaml_ng::Mapping::new();
    network::apply(&mut overrides, &fields).unwrap();
    let config = serde_yaml_ng::Value::Mapping(overrides);
    assert_eq!(config["mode"], "rule");
    assert_eq!(config["dns"]["enable"], true);
    assert_eq!(config["dns"]["enhanced-mode"], "fake-ip");
    assert_eq!(config["dns"]["respect-rules"], false);
    assert_eq!(config["dns"]["fallback-filter"]["geoip"], false);
    assert_eq!(
        config["dns"]["nameserver"][0],
        "https://223.5.5.5/dns-query"
    );
    assert_eq!(
        config["dns"]["nameserver"][1],
        "https://1.12.12.12/dns-query"
    );
    assert_eq!(
        config["dns"]["proxy-server-nameserver"][0],
        "https://223.5.5.5/dns-query"
    );
}

#[test]
fn network_values_produce_typed_nested_config_and_validate_ports() {
    let mut patch = serde_yaml_ng::Mapping::new();
    network::apply(
        &mut patch,
        &BTreeMap::from([
            ("mixed_port".into(), "17898".into()),
            ("dns".into(), "开启".into()),
            ("nameserver".into(), "1.1.1.1,8.8.8.8".into()),
            ("dns_policy".into(), "example.com=1.1.1.1;8.8.8.8".into()),
            ("tun".into(), "开启".into()),
            ("mtu".into(), "1500".into()),
        ]),
    )
    .unwrap();
    let value = serde_yaml_ng::Value::Mapping(patch.clone());
    assert_eq!(value["mixed-port"].as_u64(), Some(17898));
    assert_eq!(value["dns"]["nameserver"].as_sequence().unwrap().len(), 2);
    assert_eq!(value["tun"]["mtu"].as_u64(), Some(1500));
    assert!(network::apply(
        &mut patch,
        &BTreeMap::from([("mixed_port".into(), "70000".into())])
    )
    .is_err());
    network::apply(&mut patch, &BTreeMap::from([("dns".into(), "关闭".into())])).unwrap();
    assert!(!patch.contains_key(serde_yaml_ng::Value::from("dns")));
}
#[tokio::test]
async fn gnome_proxy_changes_and_restore_use_an_isolated_settings_database() {
    let dir = tempfile::tempdir().unwrap();
    let proxy = SystemProxy::isolated(dir.path().join("data"), dir.path().join("gnome"));
    assert_eq!(proxy.status("127.0.0.1", 17897).await.unwrap(), "关闭");
    proxy
        .enable("127.0.0.1", 17897, "localhost;127.*", None)
        .await
        .unwrap();
    assert_eq!(proxy.status("127.0.0.1", 17897).await.unwrap(), "开启");
    proxy
        .enable(
            "127.0.0.1",
            17898,
            "localhost",
            Some("file:///tmp/test.pac"),
        )
        .await
        .unwrap();
    assert_eq!(proxy.status("127.0.0.1", 17898).await.unwrap(), "开启");
    proxy.guard().await.unwrap();
    proxy.restore_if_owned().await.unwrap();
    assert_eq!(proxy.status("127.0.0.1", 17897).await.unwrap(), "关闭");
    assert!(!dir.path().join("data/system-proxy-backup.json").exists());
}

#[test]
fn pac_generation_preserves_bypass_entries_and_escapes_proxy_text() {
    let script = clash_verge_tui::platform::pac_script(
        "127.0.0.1",
        17897,
        "localhost;192.168.*;*.example.test",
    );
    assert!(script.contains("192.168.*"));
    assert!(script.contains("*.example.test"));
    assert!(script.contains("PROXY 127.0.0.1:17897; DIRECT"));
}
