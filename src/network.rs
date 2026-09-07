use anyhow::{anyhow, bail, Result};
use serde_yaml_ng::{Mapping, Value};
use std::collections::BTreeMap;

pub const DEFAULT_MIXED_PORT: u16 = 7890;
pub const DEFAULT_MIXED_PORT_TEXT: &str = "7890";
pub const TUN_SETTING_KEYS: &[&str] = &[
    "tun",
    "tun_stack",
    "tun_device",
    "auto_route",
    "strict_route",
    "auto_redirect",
    "detect_interface",
    "dns_hijack",
    "mtu",
    "exclude_route",
];

fn set(map: &mut Mapping, path: &[&str], value: Value) {
    if path.len() == 1 {
        map.insert(path[0].into(), value);
    } else {
        let child = map
            .entry(path[0].into())
            .or_insert(Value::Mapping(Mapping::new()));
        if !child.is_mapping() {
            *child = Value::Mapping(Mapping::new());
        }
        set(child.as_mapping_mut().unwrap(), &path[1..], value);
    }
}
fn list(value: &str) -> Value {
    Value::Sequence(
        value
            .split([',', ';', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string().into())
            .collect(),
    )
}
fn mapping(value: &str) -> Result<Value> {
    if value.trim().is_empty() {
        return Ok(Value::Mapping(Mapping::new()));
    }
    if let Ok(value) = serde_yaml_ng::from_str::<Value>(value) {
        if value.is_mapping() {
            return Ok(value);
        }
    }
    let mut map = Mapping::new();
    for line in value
        .split(['\n', ','])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let (k, v) = line
            .split_once('=')
            .ok_or_else(|| anyhow!("映射需要 YAML 对象或 key=value 格式"))?;
        let value = if v.contains(';') {
            list(v)
        } else {
            Value::String(v.trim().into())
        };
        map.insert(k.trim().into(), value);
    }
    Ok(Value::Mapping(map))
}
pub fn apply(overrides: &mut Mapping, fields: &BTreeMap<String, String>) -> Result<()> {
    let dns_disabled = fields.get("dns").is_some_and(|v| v == "关闭");
    if dns_disabled {
        overrides.remove(Value::from("dns"));
    }
    for (key, value) in fields {
        if key == "controller_addr"
            && !value.is_empty()
            && value.parse::<std::net::SocketAddr>().is_err()
        {
            bail!("控制器地址必须为 IP:端口");
        }
        let (path, kind): (&[&str], u8) = match key.as_str() {
            "mode" => (&["mode"], 0),
            "allow_lan" => (&["allow-lan"], 1),
            "bind_address" => (&["bind-address"], 0),
            "interface" => (&["interface-name"], 0),
            "ipv6" => (&["ipv6"], 1),
            "unified_delay" => (&["unified-delay"], 1),
            "log_level" => (&["log-level"], 0),
            "mixed_port" => (&["mixed-port"], 2),
            "socks_port" => (&["socks-port"], 2),
            "http_port" => (&["port"], 2),
            "redir_port" => (&["redir-port"], 2),
            "tproxy_port" => (&["tproxy-port"], 2),
            "tun" => (&["tun", "enable"], 1),
            "tun_stack" => (&["tun", "stack"], 0),
            "tun_device" => (&["tun", "device"], 0),
            "auto_route" => (&["tun", "auto-route"], 1),
            "strict_route" => (&["tun", "strict-route"], 1),
            "auto_redirect" => (&["tun", "auto-redirect"], 1),
            "detect_interface" => (&["tun", "auto-detect-interface"], 1),
            "dns_hijack" => (&["tun", "dns-hijack"], 3),
            "mtu" => (&["tun", "mtu"], 2),
            "exclude_route" => (&["tun", "route-exclude-address"], 3),
            "dns" => (&["dns", "enable"], 1),
            "dns_listen" => (&["dns", "listen"], 0),
            "dns_mode" => (&["dns", "enhanced-mode"], 0),
            "fake_range" => (&["dns", "fake-ip-range"], 0),
            "fake_range6" => (&["dns", "fake-ip-range6"], 0),
            "fake_filter_mode" => (&["dns", "fake-ip-filter-mode"], 0),
            "dns_ipv6" => (&["dns", "ipv6"], 1),
            "prefer_h3" => (&["dns", "prefer-h3"], 1),
            "respect_rules" => (&["dns", "respect-rules"], 1),
            "use_hosts" => (&["dns", "use-hosts"], 1),
            "system_hosts" => (&["dns", "use-system-hosts"], 1),
            "default_dns" => (&["dns", "default-nameserver"], 3),
            "nameserver" => (&["dns", "nameserver"], 3),
            "fallback" => (&["dns", "fallback"], 3),
            "proxy_dns" => (&["dns", "proxy-server-nameserver"], 3),
            "direct_dns" => (&["dns", "direct-nameserver"], 3),
            "direct_policy" => (&["dns", "direct-nameserver-follow-policy"], 1),
            "fake_filter" => (&["dns", "fake-ip-filter"], 3),
            "dns_policy" => (&["dns", "nameserver-policy"], 4),
            "geo_filter" => (&["dns", "fallback-filter", "geoip"], 1),
            "geo_code" => (&["dns", "fallback-filter", "geoip-code"], 0),
            "fallback_cidr" => (&["dns", "fallback-filter", "ipcidr"], 3),
            "fallback_domain" => (&["dns", "fallback-filter", "domain"], 3),
            "hosts" => (&["hosts"], 4),
            "cors_private" => (&["external-controller-cors", "allow-private-network"], 1),
            "cors_origins" => (&["external-controller-cors", "allow-origins"], 3),
            "geo_source" => {
                if value == "MetaCubeX" {
                    overrides.remove(Value::from("geox-url"));
                } else {
                    let urls = mapping(value)?;
                    for (_, v) in urls.as_mapping().unwrap() {
                        let u = v
                            .as_str()
                            .and_then(|s| url::Url::parse(s).ok())
                            .ok_or_else(|| anyhow!("GeoData 地址需要 HTTP(S) URL"))?;
                        if !matches!(u.scheme(), "http" | "https") {
                            bail!("GeoData 地址需要 HTTP(S) URL");
                        }
                    }
                    set(overrides, &["geox-url"], urls);
                }
                continue;
            }
            "geo_auto" => (&["geo-auto-update"], 1),
            "geo_interval" => (&["geo-update-interval"], 2),
            "tunnels" => (&["tunnels"], 5),
            "webui_path" => (&["external-ui"], 0),
            "webui" => {
                let url = if value == "Yacd" {
                    "https://github.com/MetaCubeX/Yacd-meta/archive/refs/heads/gh-pages.zip"
                } else {
                    "https://github.com/MetaCubeX/metacubexd/archive/refs/heads/gh-pages.zip"
                };
                set(overrides, &["external-ui-url"], url.into());
                continue;
            }
            _ => continue,
        };
        if dns_disabled && path.first() == Some(&"dns") {
            continue;
        }
        let parsed = match kind {
            1 => {
                if !matches!(value.as_str(), "开启" | "关闭") {
                    bail!("开关值无效");
                }
                Value::Bool(value == "开启")
            }
            2 => {
                let n = value.parse::<u64>().map_err(|_| anyhow!("数值格式无效"))?;
                if key.ends_with("_port") && (n > 65535 || (key == "mixed_port" && n == 0)) {
                    bail!("端口应在 1–65535，其他端口可用 0 禁用");
                }
                if key == "mtu" && !(576..=65535).contains(&n) {
                    bail!("MTU 应在 576–65535");
                }
                n.into()
            }
            3 => list(value),
            4 => mapping(value)?,
            5 => {
                let parsed: Value =
                    serde_yaml_ng::from_str(value).map_err(|_| anyhow!("隧道 YAML 无效"))?;
                if !parsed.is_sequence() {
                    bail!("隧道配置需要 YAML 数组");
                }
                parsed
            }
            _ => {
                if key == "interface" && value == "自动" {
                    Value::String(String::new())
                } else {
                    value.clone().into()
                }
            }
        };
        set(overrides, path, parsed);
    }
    Ok(())
}

pub async fn preflight(
    fields: &BTreeMap<String, String>,
    candidate: &Value,
    previous: &serde_json::Value,
) -> Result<()> {
    if fields.contains_key("bind_address") || fields.contains_key("allow_lan") {
        let address = candidate["bind-address"].as_str().unwrap_or("127.0.0.1");
        let address = if address == "*" { "0.0.0.0" } else { address };
        let ip = address
            .parse::<std::net::IpAddr>()
            .map_err(|_| anyhow!("绑定地址必须为本机 IP 或 *"))?;
        let _ = std::net::TcpListener::bind(std::net::SocketAddr::new(ip, 0))
            .map_err(|_| anyhow!("绑定地址不属于可用的本机接口"))?;
    }
    if fields.contains_key("dns_listen") && candidate["dns"]["enable"].as_bool().unwrap_or(false) {
        if let Some(listen) = candidate["dns"]["listen"]
            .as_str()
            .filter(|s| !s.is_empty())
        {
            if previous["dns"]["listen"].as_str() != Some(listen) {
                let address = listen
                    .parse::<std::net::SocketAddr>()
                    .map_err(|_| anyhow!("DNS 监听地址必须为 IP:端口"))?;
                let _ = std::net::UdpSocket::bind(address)
                    .map_err(|_| anyhow!("DNS 监听地址或端口不可用"))?;
            }
        }
    }
    for (field, key) in [
        ("mixed_port", "mixed-port"),
        ("socks_port", "socks-port"),
        ("http_port", "port"),
        ("redir_port", "redir-port"),
        ("tproxy_port", "tproxy-port"),
    ] {
        if !fields.contains_key(field) {
            continue;
        }
        let port = candidate[key].as_u64().unwrap_or(0);
        if port == 0 || previous[key].as_u64() == Some(port) {
            continue;
        }
        let _tcp = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port as u16))
            .map_err(|_| anyhow!("端口 {port} 已被占用"))?;
    }
    if fields.get("tun").is_some_and(|v| v == "开启")
        && !previous["tun"]["enable"].as_bool().unwrap_or(false)
    {
        let name = candidate["tun"]["device"].as_str().unwrap_or("Meta");
        if interface_exists(name).await? {
            bail!("TUN 网卡名称已被占用，请使用独立名称");
        }
    }
    Ok(())
}
async fn interface_exists(name: &str) -> Result<bool> {
    let status = tokio::process::Command::new("ip")
        .args(["link", "show", "dev", name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|_| anyhow!("TUN 验证需要 Linux iproute2"))?;
    Ok(status.success())
}
pub async fn verify_tun_state(name: &str, expected: bool) -> Result<()> {
    for _ in 0..50 {
        if interface_exists(name).await? == expected {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let state = tokio::process::Command::new("ip")
        .args(["-brief", "link", "show", "dev", name])
        .output()
        .await
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|output| !output.is_empty())
        .unwrap_or_else(|| "不存在".into());
    bail!(
        "TUN 网卡未在 5 秒内{}（当前：{state}）；请检查权限服务和 core.log",
        if expected { "创建" } else { "移除" }
    )
}

pub async fn verify_tun(fields: &BTreeMap<String, String>, candidate: &Value) -> Result<()> {
    let Some(value) = fields.get("tun") else {
        return Ok(());
    };
    let name = candidate["tun"]["device"].as_str().unwrap_or("Meta");
    verify_tun_state(name, value == "开启")
        .await
        .map_err(|error| anyhow!("{error}，原配置已恢复"))
}
