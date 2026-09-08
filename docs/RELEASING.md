# 版本维护

首版：`v0.1.0`。主分支：`master`。使用语义化版本和带注释的标签；已发布标签不移动。

## 迭代流程

1. 提交说明统一为 `v主版本.次版本.补丁版本: English summary`，使用简短英文，例如 `v0.1.5: Compact content lists and settings forms`。同一版本内的多个提交使用相同版本号，不使用 `feat:`、`fix:` 等类型前缀。
2. 新版本修改 `Cargo.toml` 的 `version`，运行 Cargo 更新 `Cargo.lock`。
3. 将 CHANGELOG 的未发布内容归入新版本，同步 英文 `CHANGELOG.md`。
4. 使用方式或功能范围变化时同步 `README.md` 与 `README.zh-CN.md`。
5. 界面变化后重新生成并检查 `docs/previews`。
6. Linux 发行变更使用临时 HOME 测试组合包、安装脚本和 `--check`，确认固定内核版本与文件权限。
7. 检查通过后提交，确认工作区干净，创建版本标签。

```bash
# 发布前检查
sh -n scripts/install.sh
python3 scripts/test-install.py
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release

# 核对记录
./target/release/clash-verge-tui --version
git status --short
git log --oneline -5

# 使用本次版本号与简短英文摘要提交
VERSION=v0.1.6
git commit -m "$VERSION: Summarize the update"

# 版本提交完成后建立带注释的标签
git tag -a "$VERSION" -m "Release $VERSION"

# 推送分支与该版本标签
git push origin master "$VERSION"
```

GitHub Actions 在推送和 Pull Request 时执行格式、Clippy、测试与构建。正式标签额外触发 Release 工作流：分别在 Linux x86_64/aarch64 runner 使用 musl-tools 静态构建，下载并校验代码固定的官方 Mihomo 资产，通过 readelf 检查 TUI 与内核无动态加载器、共享库依赖或 GLIBC 符号，附带 musl 许可，生成组合包与 SHA-256 文件，在 Ubuntu 20.04 容器完成真实安装、配置权限、内核修复与退出清理检查，并随 `install.sh` 创建 GitHub Release。

```bash
# 本地检查 x86_64 发行包；VERSION 不带 v
VERSION=1.4.2
# 安装 musl-tools 后构建静态发行包
rustup target add x86_64-unknown-linux-musl
RUSTFLAGS="-C target-feature=+crt-static -C linker=musl-gcc" cargo build --locked --release --target x86_64-unknown-linux-musl
TARGET=x86_64-unknown-linux-musl ARCHITECTURE=x86_64 APP_VERSION="$VERSION" \
  ./scripts/package-linux.sh

# Release 发布后验证公开一键安装
curl -fsSL https://github.com/wty-yy/clash-verge-tui/releases/latest/download/install.sh | sh
clash-verge-tui --check
```

远端为 `https://github.com/wty-yy/clash-verge-tui.git`，默认分支为 `master`。Release 必须同时包含两个架构的 `.tar.gz`、对应 `.sha256` 和 `install.sh`。Mihomo 版本变化时，同步修改应用常量、两种架构的压缩包/二进制哈希、打包脚本、README、CHANGELOG 与验证记录。

补丁版本用于兼容修复；次版本用于新增能力或预发布阶段接口调整。`v1.0.0` 前需完成明确的真实功能验收，不能以演示状态作为网络功能验收结果。

可先手动触发 Release 工作流（workflow_dispatch）验证 master 的双架构构建与安装；手动运行只上传 Actions 构建产物，正式标签才创建 GitHub Release。

Linux 组合包同时包含固定 GeoSite 快照。更新快照时，同步 `src/core_manager.rs` 与 `scripts/package-linux.sh` 的 SHA-256，以及打包脚本中的上游提交地址；保留数据许可。`scripts/test-release.py` 在禁止 GeoSite 下载的配置下验证首次启动与数据补齐。

## Gitee 镜像发行

镜像仓库为 `https://gitee.com/wty-yy/clash-verge-tui`。Git 同步只复制源码和标签，不复制 Release 附件；当前 Release 工作流只发布 GitHub。

1. 同步包含新版 `scripts/install.sh` 的 `master` 分支及版本标签
2. 在 Gitee 创建同标签的发行版，上传 GitHub Release 中同一份 x86_64 / aarch64 `.tar.gz`、各自 `.tar.gz.sha256` 和 `install.sh`
3. 核对两个来源归档的 SHA-256 一致，再执行 Gitee 一键安装和 `--check`

```bash
# 从 Gitee 脚本与 Gitee Release 安装
curl -fsSL https://gitee.com/wty-yy/clash-verge-tui/raw/master/scripts/install.sh | sh -s -- --source gitee

# 指定已经发布到 Gitee 的版本；环境变量放在 sh 前
curl -fsSL https://gitee.com/wty-yy/clash-verge-tui/raw/master/scripts/install.sh | CLASH_VERGE_TUI_VERSION=v1.4.2 sh -s -- --source gitee
clash-verge-tui --check
```

Gitee 版本发现使用 `/api/v5/repos/{owner}/{repo}/releases/latest`，附件使用 `/releases/download/{tag}/{filename}`。源码同步后若发行版或附件尚未上传，安装脚本明确报错，不回退至 GitHub。
