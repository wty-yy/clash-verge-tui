use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub enum Kind {
    Text,
    Secret,
    Number,
    Toggle,
    Choice(&'static [&'static str]),
    Multiline,
}
#[derive(Clone, Debug)]
pub struct Spec {
    pub key: &'static str,
    pub label: &'static str,
    pub default: &'static str,
    pub kind: Kind,
}
#[derive(Clone, Debug)]
pub struct Section {
    pub category: usize,
    pub name: &'static str,
    pub description: &'static str,
    pub fields: Vec<Spec>,
}
fn f(key: &'static str, label: &'static str, default: &'static str, kind: Kind) -> Spec {
    Spec {
        key,
        label,
        default,
        kind,
    }
}
use Kind::*;
pub const CATEGORIES: [&str; 5] = ["系统", "内核", "界面", "高级", "关于"];
pub fn sections() -> Vec<Section> {
    vec![
 Section{category:0,name:"系统代理",description:"代理主机、PAC、绕过与守卫",fields:vec![f("system_proxy","系统代理","关闭",Toggle),f("proxy_host","代理主机","127.0.0.1",Text),f("pac","PAC 模式","关闭",Toggle),f("guard","代理守卫","开启",Toggle),f("guard_interval","守卫间隔 / 秒","30",Number),f("bypass","代理绕过","localhost;127.*;192.168.*;10.*",Text),f("pac_script","PAC 脚本（留空自动生成）","",Multiline)]},
 Section{category:0,name:"虚拟网卡 TUN",description:"协议栈、路由、DNS 劫持与 MTU",fields:vec![f("tun","虚拟网卡模式","关闭",Toggle),f("tun_stack","协议栈","mixed",Choice(&["mixed","system","gvisor"])),f("tun_device","网卡名称","cvtun0",Text),f("auto_route","自动路由","开启",Toggle),f("strict_route","严格路由","关闭",Toggle),f("auto_redirect","自动重定向","开启",Toggle),f("detect_interface","自动选择出口","开启",Toggle),f("dns_hijack","DNS 劫持","any:53",Text),f("mtu","MTU","9000",Number),f("exclude_route","排除网段","192.168.0.0/16",Text)]},
 Section{category:0,name:"启动与服务",description:"开机启动、后台服务与静默启动",fields:vec![f("auto_launch","登录后自启","关闭",Toggle),f("silent","静默启动","关闭",Toggle),f("service","后台服务","未安装",Choice(&["未安装","运行中","已停止"])),f("start_script","启动脚本","",Multiline)]},
 Section{category:1,name:"基础网络",description:"局域网、IPv6、统一延迟、日志等级",fields:vec![f("allow_lan","局域网连接","关闭",Toggle),f("bind_address","绑定地址","127.0.0.1",Text),f("interface","出口接口","自动",Text),f("ipv6","IPv6","关闭",Toggle),f("unified_delay","统一延迟","开启",Toggle),f("log_level","日志等级","info",Choice(&["debug","info","warning","error","silent"]))]},
 Section{category:1,name:"端口设置",description:"Mixed、SOCKS、HTTP、Redir、TProxy",fields:vec![f("mixed_port","混合代理端口","7897",Number),f("socks_port","SOCKS 端口（0 关闭）","0",Number),f("http_port","HTTP 端口（0 关闭）","0",Number),f("redir_port","Redir 端口（0 关闭）","0",Number),f("tproxy_port","TProxy 端口（0 关闭）","0",Number)]},
 Section{category:1,name:"DNS 覆写",description:"解析模式、服务器、Fake IP 与过滤策略",fields:vec![f("dns","启用 DNS 覆写","关闭",Toggle),f("dns_listen","监听地址","127.0.0.1:1053",Text),f("dns_mode","增强模式","fake-ip",Choice(&["fake-ip","redir-host"])),f("fake_range","Fake IP 范围","198.18.0.1/16",Text),f("fake_range6","Fake IP IPv6 范围","fc00::/18",Text),f("fake_filter_mode","Fake IP 过滤模式","blacklist",Choice(&["blacklist","whitelist"])),f("dns_ipv6","IPv6 解析","关闭",Toggle),f("prefer_h3","优先 HTTP/3","关闭",Toggle),f("respect_rules","遵循路由规则","开启",Toggle),f("use_hosts","使用 Hosts","开启",Toggle),f("system_hosts","使用系统 Hosts","开启",Toggle),f("default_dns","默认服务器","223.5.5.5",Text),f("nameserver","域名服务器","https://dns.alidns.com/dns-query",Text),f("fallback","回退服务器","https://1.1.1.1/dns-query",Text),f("proxy_dns","代理节点 DNS","223.5.5.5",Text),f("direct_dns","直连 DNS","system",Text),f("direct_policy","直连遵循策略","关闭",Toggle),f("fake_filter","Fake IP 过滤","*.lan,localhost",Text),f("dns_policy","域名策略","",Multiline),f("geo_filter","GeoIP 过滤","开启",Toggle),f("geo_code","GeoIP 国家代码","CN",Text),f("fallback_cidr","回退 IP CIDR","240.0.0.0/4",Text),f("fallback_domain","回退域名","",Text),f("hosts","Hosts 映射","",Multiline)]},
 Section{category:1,name:"外部控制器",description:"控制地址、访问密钥与跨域设置",fields:vec![f("controller","启用外部控制器","关闭",Toggle),f("controller_addr","监听地址","127.0.0.1:9090",Text),f("secret","API 密钥","",Secret),f("cors_private","允许专用网络","关闭",Toggle),f("cors_origins","允许来源","http://localhost",Text)]},
 Section{category:1,name:"内核与 GeoData",description:"内核通道、GeoData 来源与更新偏好",fields:vec![f("core_channel","内核通道","Stable",Choice(&["Stable","Alpha"])),f("geo_auto","GeoData 自动更新","开启",Toggle),f("geo_interval","更新间隔 / 小时","24",Number),f("geo_source","MetaCubeX 或 geox-url YAML","MetaCubeX",Multiline)]},
 Section{category:1,name:"网页界面",description:"外部面板地址与界面资源",fields:vec![f("webui","面板","MetaCubeXD",Choice(&["MetaCubeXD","Yacd"])),f("webui_url","面板地址","http://127.0.0.1:9090/ui",Text),f("webui_path","界面目录","ui",Text)]},
 Section{category:1,name:"流量隧道",description:"隧道列表编辑（YAML），支持多条映射",fields:vec![f("tunnels","隧道配置","# 演示隧道列表\n- network: [tcp, udp]\n  address: 127.0.0.1:5353\n  target: 1.1.1.1:53\n  proxy: DIRECT",Multiline)]},
 Section{category:2,name:"外观与布局",description:"主题、强调色、导航与流量图",fields:vec![f("theme","主题","深色",Choice(&["深色","浅色"])),f("accent","强调色","紫色",Choice(&["紫色","青色","蓝色"])),f("compact","紧凑导航","关闭",Toggle),f("traffic_graph","流量图","开启",Toggle),f("memory","内核占用","开启",Toggle),f("start_page","启动页面","首页",Choice(&["首页","代理","订阅","连接","规则","日志","解锁检测","设置"]))]},
 Section{category:2,name:"热键与终端",description:"鼠标操作与交互偏好",fields:vec![f("mouse","鼠标操作","开启",Toggle),f("vim","Vim 导航 j/k","开启",Toggle),f("env_type","环境变量类型","bash",Choice(&["bash","zsh","fish","powershell"])),f("refresh","刷新间隔 / 毫秒","1000",Number)]},
 Section{category:2,name:"杂项设置",description:"延迟测试、连接清理与日志保留",fields:vec![f("close_connections","切换节点清理连接","关闭",Toggle),f("auto_check","自动检查更新","开启",Toggle),f("enhance","启用全局增强","开启",Toggle),f("auto_delay","自动延迟检测","关闭",Toggle),f("delay_interval","测速间隔 / 秒","300",Number),f("test_url","测试链接","https://www.gstatic.com/generate_204",Text),f("timeout","测试超时 / 毫秒","5000",Number),f("log_size","日志大小 / MB","10",Number),f("log_count","日志文件数量","7",Number),f("log_clean","自动轮转日志","开启",Toggle)]},
 Section{category:3,name:"备份与恢复",description:"本地演示快照；WebDAV 配置表单",fields:vec![f("auto_backup","自动备份","关闭",Toggle),f("backup_password","备份加密密码（可留空）","",Secret),f("webdav_url","WebDAV 地址","",Text),f("webdav_user","用户名","",Text),f("webdav_password","密码","",Secret)]},
 Section{category:3,name:"运行配置",description:"查看组合后的演示配置",fields:vec![]},
 Section{category:3,name:"诊断与目录",description:"状态路径、版本和诊断信息",fields:vec![]},
 Section{category:3,name:"轻量模式",description:"后台保留与空闲刷新设置",fields:vec![f("lite","自动轻量模式","关闭",Toggle),f("lite_delay","空闲延迟 / 秒","60",Number)]},
 Section{category:4,name:"关于 Clash Verge TUI",description:"终端客户端 · MIT",fields:vec![]},
 Section{category:4,name:"桌面功能映射",description:"桌面专有功能的终端适配说明",fields:vec![]},
 ]
}
pub fn defaults() -> BTreeMap<String, String> {
    sections()
        .into_iter()
        .flat_map(|s| s.fields.into_iter())
        .map(|s| (s.key.into(), s.default.into()))
        .collect()
}
