use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Home,
    Proxies,
    Profiles,
    Connections,
    Rules,
    Logs,
    Unlock,
    Settings,
}
impl Page {
    pub const ALL: [Self; 8] = [
        Self::Home,
        Self::Proxies,
        Self::Profiles,
        Self::Connections,
        Self::Rules,
        Self::Logs,
        Self::Unlock,
        Self::Settings,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap()
    }
    pub fn title(self) -> &'static str {
        [
            "首页",
            "代理",
            "订阅",
            "连接",
            "规则",
            "日志",
            "解锁检测",
            "设置",
        ][self.index()]
    }
    pub fn slug(self) -> &'static str {
        [
            "home",
            "proxies",
            "profiles",
            "connections",
            "rules",
            "logs",
            "unlock",
            "settings",
        ][self.index()]
    }
    pub fn description(self) -> &'static str {
        [
            "网络概览与快捷控制",
            "策略组与节点管理",
            "订阅配置与增强链",
            "实时会话与流量去向",
            "路由规则与规则集合",
            "内核日志与事件追踪",
            "流媒体与 AI 服务可用性",
            "系统、内核与界面偏好",
        ][self.index()]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub protocol: String,
    pub region: String,
    pub delay: Option<u16>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    pub kind: String,
    pub selected: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub url: String,
    pub interval: String,
    pub used: u16,
    pub total: u16,
    pub updated: String,
    pub content: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Enhancement {
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub content: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Connection {
    pub id: u32,
    pub host: String,
    pub process: String,
    pub network: String,
    pub chain: String,
    pub rule: String,
    pub down: String,
    pub up: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub kind: String,
    pub payload: String,
    pub target: String,
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Log {
    pub time: String,
    pub level: String,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Unlock {
    pub name: String,
    pub category: String,
    pub result: String,
    pub region: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Backup {
    pub name: String,
    pub settings: BTreeMap<String, String>,
    pub profiles: Vec<Profile>,
    pub active_profile: usize,
    pub enhancements: Vec<Enhancement>,
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DemoState {
    pub schema: u32,
    pub mode: usize,
    pub active_profile: usize,
    pub nodes: Vec<Node>,
    pub groups: Vec<Group>,
    pub profiles: Vec<Profile>,
    pub enhancements: Vec<Enhancement>,
    pub connections: Vec<Connection>,
    pub rules: Vec<Rule>,
    pub logs: Vec<Log>,
    pub unlocks: Vec<Unlock>,
    pub settings: BTreeMap<String, String>,
    pub backups: Vec<Backup>,
}
impl Default for DemoState {
    fn default() -> Self {
        let nodes = [
            ("香港 · Central 01", "VLESS", "HK", Some(28)),
            ("香港 · Central 02", "Trojan", "HK", Some(35)),
            ("日本 · Tokyo 01", "VLESS", "JP", Some(62)),
            ("日本 · Osaka 02", "Hysteria2", "JP", Some(74)),
            ("新加坡 · Marina 01", "Trojan", "SG", Some(83)),
            ("美国 · San Jose 01", "VMess", "US", Some(156)),
            ("德国 · Frankfurt 01", "Shadowsocks", "DE", Some(188)),
            ("英国 · London 01", "VLESS", "GB", None),
        ]
        .into_iter()
        .map(|(name, protocol, region, delay)| Node {
            name: name.into(),
            protocol: protocol.into(),
            region: region.into(),
            delay,
        })
        .collect();
        let content = "# 演示配置 · 不会写入 mihomo\nmode: rule\nmixed-port: 7897\nallow-lan: false\nlog-level: info\n".to_string();
        Self {
            schema: 1,
            mode: 0,
            active_profile: 0,
            nodes,
            groups: [
                ("节点选择", "select", "香港 · Central 01"),
                ("自动选择", "url-test", "香港 · Central 02"),
                ("流媒体", "select", "日本 · Tokyo 01"),
                ("AI 服务", "select", "美国 · San Jose 01"),
            ]
            .into_iter()
            .map(|(name, kind, selected)| Group {
                name: name.into(),
                kind: kind.into(),
                selected: selected.into(),
            })
            .collect(),
            profiles: vec![
                Profile {
                    name: "日常订阅".into(),
                    url: "https://example.com/daily.yaml".into(),
                    interval: "720".into(),
                    used: 42,
                    total: 200,
                    updated: "演示 · 尚未刷新".into(),
                    content: content.clone(),
                },
                Profile {
                    name: "备用线路".into(),
                    url: "https://example.com/backup.yaml".into(),
                    interval: "1440".into(),
                    used: 8,
                    total: 100,
                    updated: "演示 · 尚未刷新".into(),
                    content,
                },
            ],
            enhancements: vec![
                Enhancement {
                    name: "全局扩展配置".into(),
                    kind: "YAML".into(),
                    enabled: true,
                    content: "# 全局覆写\nprofile:\n  store-selected: true\n".into(),
                },
                Enhancement {
                    name: "全局扩展脚本".into(),
                    kind: "JavaScript".into(),
                    enabled: true,
                    content: "function main(config) {\n  return config;\n}\n".into(),
                },
            ],
            connections: [
                (
                    "api.github.com",
                    "git",
                    "TCP",
                    "节点选择 → 香港",
                    "DOMAIN-SUFFIX",
                    "24.8 MiB",
                    "128 KiB",
                ),
                (
                    "www.youtube.com",
                    "firefox",
                    "QUIC",
                    "流媒体 → 日本",
                    "RULE-SET",
                    "182.3 MiB",
                    "1.2 MiB",
                ),
                (
                    "api.openai.com",
                    "terminal",
                    "TCP",
                    "AI 服务 → 美国",
                    "DOMAIN-SUFFIX",
                    "3.6 MiB",
                    "842 KiB",
                ),
                (
                    "registry.npmjs.org",
                    "node",
                    "TCP",
                    "节点选择 → 香港",
                    "RULE-SET",
                    "12.4 MiB",
                    "32 KiB",
                ),
                (
                    "192.168.1.1",
                    "system",
                    "UDP",
                    "DIRECT",
                    "IP-CIDR",
                    "6.2 KiB",
                    "2.1 KiB",
                ),
                (
                    "example.com",
                    "curl",
                    "TCP",
                    "DIRECT",
                    "MATCH",
                    "8.4 KiB",
                    "1.0 KiB",
                ),
            ]
            .into_iter()
            .enumerate()
            .map(
                |(i, (host, process, network, chain, rule, down, up))| Connection {
                    id: i as u32 + 1,
                    host: host.into(),
                    process: process.into(),
                    network: network.into(),
                    chain: chain.into(),
                    rule: rule.into(),
                    down: down.into(),
                    up: up.into(),
                },
            )
            .collect(),
            rules: [
                ("DOMAIN-SUFFIX", "github.com", "节点选择"),
                ("DOMAIN-SUFFIX", "openai.com", "AI 服务"),
                ("RULE-SET", "streaming", "流媒体"),
                ("RULE-SET", "reject", "REJECT"),
                ("GEOIP", "CN", "DIRECT"),
                ("IP-CIDR", "192.168.0.0/16", "DIRECT"),
                ("MATCH", "*", "节点选择"),
            ]
            .into_iter()
            .map(|(kind, payload, target)| Rule {
                kind: kind.into(),
                payload: payload.into(),
                target: target.into(),
                enabled: true,
            })
            .collect(),
            logs: [
                ("INFO", "界面演示已就绪；尚未连接 mihomo"),
                ("INFO", "加载演示订阅：日常订阅"),
                ("DEBUG", "配置增强链：YAML → JavaScript → 运行配置"),
                ("INFO", "[TCP] api.github.com → 节点选择 / 香港"),
                ("WARN", "[演示] London 01 延迟测试超时"),
                ("ERROR", "[演示] 示例错误：备用订阅更新失败"),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (level, message))| Log {
                time: format!("12:00:{i:02}"),
                level: level.into(),
                message: message.into(),
            })
            .collect(),
            unlocks: [
                ("Netflix", "流媒体"),
                ("YouTube Premium", "流媒体"),
                ("Disney+", "流媒体"),
                ("Spotify", "流媒体"),
                ("TikTok", "社交"),
                ("ChatGPT", "AI"),
                ("Claude", "AI"),
                ("Gemini", "AI"),
            ]
            .into_iter()
            .map(|(name, category)| Unlock {
                name: name.into(),
                category: category.into(),
                result: "未检测".into(),
                region: "—".into(),
            })
            .collect(),
            settings: crate::settings::defaults(),
            backups: Vec::new(),
        }
    }
}
impl DemoState {
    pub fn value(&self, key: &str) -> &str {
        self.settings.get(key).map(String::as_str).unwrap_or("")
    }
    pub fn toggle(&mut self, key: &str) {
        let value = if self.value(key) == "开启" {
            "关闭"
        } else {
            "开启"
        };
        self.settings.insert(key.into(), value.into());
    }
    pub fn active_name(&self) -> &str {
        self.profiles
            .get(self.active_profile)
            .map(|p| p.name.as_str())
            .unwrap_or("未选择订阅")
    }
}
