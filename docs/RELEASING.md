# 版本维护

首版：`v0.1.0`。主分支：`main`。使用语义化版本和带注释的标签；已发布标签不移动。

## 迭代流程

1. 按功能、修复、文档组织提交：`feat:`、`fix:`、`test:`、`docs:`、`chore:`。
2. 新版本修改 `Cargo.toml` 的 `version`，运行 Cargo 更新 `Cargo.lock`。
3. 将 CHANGELOG 的未发布内容归入新版本，同步 `CHANGELOG.md` 与 `CHANGELOG.zh-CN.md`。
4. 使用方式或功能范围变化时同步 `README.md` 与 `README.zh-CN.md`。
5. 界面变化后重新生成并检查 `docs/previews`。
6. 检查通过后提交，确认工作区干净，创建版本标签。

```bash
# 发布前检查
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release

# 核对记录
./target/release/clash-verge-tui --version
git status --short
git log --oneline -5

# 版本提交完成后建立带注释的标签；替换为本次版本
VERSION=v0.1.1
git tag -a "$VERSION" -m "Release $VERSION"
```

GitHub Actions 在推送和 Pull Request 时执行格式、Clippy、测试与构建。当前仓库只维护本地提交与标签，未配置远端发布流程。

补丁版本用于兼容修复；次版本用于新增能力或预发布阶段接口调整。`v1.0.0` 前需完成明确的真实功能验收，不能以演示状态作为网络功能验收结果。
