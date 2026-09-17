# clash-verge-tui.wty-yy.top GitHub 镜像

[English](README.md) · **简体中文**

`worker.js` 是部署在 Cloudflare Workers 上的透明下载代理：把本仓库 GitHub Release 的资产（`install.sh`、`.tar.gz`、`.sha256`）以及固定版本的 MetaCubeX GeoData、网页界面与 mihomo 内核通过 `https://clash-verge-tui.wty-yy.top` 分发并做边缘缓存。该脚本不存储文件、不调用 GitHub API，只转发所配置仓库的 release 资产与显式白名单资产。

## 变量

| 变量 | 示例 | 说明 |
| --- | --- | --- |
| `MIRROR_DOMAIN` | `clash-verge-tui.wty-yy.top` | 镜像对外域名，用于页面提示与 `x-mirror` 响应头 |
| `ORIGIN` | `https://github.com/wty-yy/clash-verge-tui` | 上游仓库地址，只允许转发该仓库 `releases/` 下的资产 |

两个变量都有同值默认；`wrangler.json` 的 `vars` 已填好，Dashboard 部署时在 Settings → Variables and Secrets 中同样填入这两项即可。

## 支持的地址

| 形式 | 目标 |
| --- | --- |
| `/https://github.com/<owner>/<repo>/releases/download/<tag>/<asset>` | 与 gh-proxy 前缀约定兼容，`--github-proxy https://clash-verge-tui.wty-yy.top` 直接可用 |
| `/<owner>/<repo>/releases/download/<tag>/<asset>` | 去掉前缀的常规路径 |
| `/install.sh` | 最新版本的安装脚本，缓存 5 分钟 |
| `/latest/<asset>` | 由 GitHub 解析最新版本，缓存 5 分钟 |
| `/v<major>.<minor>.<patch>/<asset>` | 指定版本，缓存 1 年 |
| `/geodata/<geoip.dat\|geosite.dat\|geoip.metadb\|country.mmdb\|GeoLite2-ASN.mmdb>` | MetaCubeX/meta-rules-dat `latest` release 资产，缓存 5 分钟 |
| `/ui/<metacubexd\|yacd-meta>.tar.gz` | 面板 `gh-pages` 分支的网页资源归档，缓存 1 年 |
| `/core/v<版本>/mihomo-linux-<amd64\|arm64>-v<版本>.gz` | 固定版本 mihomo 内核压缩包，缓存 1 年 |
| `/latest-version` | 由 GitHub 跳转解析出的最新版本标签，缓存 5 分钟 |

`/health` 返回 `ok`，`/` 返回用法说明。

## 部署

使用 Dashboard：

1. Workers & Pages → Create application → Create Worker，名称 `gh-mirror`。
2. Edit code 中全量粘贴 `worker.js`，Deploy。
3. Settings → Domains & Routes → Add → Custom domain，填 `clash-verge-tui.wty-yy.top`（`wty-yy.top` 已托管在 Cloudflare，DNS 记录与证书会自动创建，不要手工添加 A/CNAME）。
4. Settings → Variables and Secrets 添加 `MIRROR_DOMAIN` 与 `ORIGIN`（可留空使用默认值）。
5. 建议在 SSL/TLS 中开启 Always Use HTTPS。

使用 Wrangler：

```bash
# 在 deploy/gh-mirror 下登录并部署；wrangler.json 已包含两个变量和自定义域
npx wrangler login
npx wrangler deploy
```

## 验证

```bash
# 健康检查
curl -fsSL https://clash-verge-tui.wty-yy.top/health

# 下载校验文件（小文件，适合检查连通性）
curl -fsSL https://clash-verge-tui.wty-yy.top/v1.5.0/clash-verge-tui-v1.5.0-linux-x86_64.tar.gz.sha256

# 经镜像引导安装：脚本与归档都走镜像，归档仍校验发布版 SHA-256
curl -fsSL https://clash-verge-tui.wty-yy.top/install.sh | sh -s -- --source proxy

# TUI 运行时资产（GeoData、网页界面与最新版本标签）
curl -fsSL https://clash-verge-tui.wty-yy.top/geodata/geoip.metadb -o /dev/null
curl -fsSL https://clash-verge-tui.wty-yy.top/ui/metacubexd.tar.gz -o /dev/null
curl -fsSL https://clash-verge-tui.wty-yy.top/latest-version
```

发行版 `install.sh` 的 `release_version` 由 Release 工作流在 publish 阶段写入标签版本。

## 测试

```bash
node --test deploy/gh-mirror/*.test.mjs
```

## 说明

- Worker 免费版每天 10 万次请求，命中缓存不重复回源 GitHub；首次下载由 Cloudflare 回源，之后从边缘返回。
- `latest` 路径缓存 300 秒，具体版本路径缓存 1 年，因此重复安装不消耗 GitHub 带宽。
- 未匹配的路径返回 404，Worker 不会成为开放代理。
