# 版本维护

首版：`v0.1.0`。主分支：`master`。使用语义化版本和带注释的标签；已发布标签不移动。

## 迭代流程

1. 提交说明统一为 `v主版本.次版本.补丁版本: English summary`，使用简短英文，例如 `v0.1.5: Compact content lists and settings forms`。同一版本内的多个提交使用相同版本号，不使用 `feat:`、`fix:` 等类型前缀。
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

# 使用本次版本号与简短英文摘要提交
VERSION=v0.1.6
git commit -m "$VERSION: Summarize the update"

# 版本提交完成后建立带注释的标签
git tag -a "$VERSION" -m "Release $VERSION"

# 推送分支与该版本标签
git push origin master "$VERSION"
```

GitHub Actions 在推送和 Pull Request 时执行格式、Clippy、测试与构建。远端为 `https://github.com/wty-yy/clash-verge-tui.git`，默认分支为 `master`；推送分支与版本标签，不自动创建 GitHub Release。

补丁版本用于兼容修复；次版本用于新增能力或预发布阶段接口调整。`v1.0.0` 前需完成明确的真实功能验收，不能以演示状态作为网络功能验收结果。
