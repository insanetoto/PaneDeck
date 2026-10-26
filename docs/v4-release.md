# PaneDeck v4 测试版交付状态（PD-340）

> 记录日期：2026-10-23
> 任务：[tasks-v4.md](tasks-v4.md) · 验收：[v4-acceptance.md](v4-acceptance.md)
> 状态：**不适用**（用户 2026-10-23 明确不考虑 GitHub 发布链路）；保留本地构建方式与未测边界，不创建流水线、不发布标签。

## 1. 当前已有

- 前端：`npm run build:web` 通过，生成 `dist/`。
- 原生：`npm run build`（`tauri build --no-bundle`）通过，生成 `target/release/panedeck`。
- 质量门：前端 22 个文件 / 115 项；Rust 139 项通过、2 项真实废纸篓测试忽略；fmt/Clippy、边界与范围检查通过。
- 只读原生证据：release 二进制可包入 ad-hoc `.app`，经 LaunchServices（`open`）启动后读取 iCloud 和本地卷；过滤前的收藏截图见 [native-window.png](artifacts/v4/native-window.png)，当前无完全磁盘访问时隐藏共享收藏的警告见 [native-window-visibility-warning.png](artifacts/v4/native-window-visibility-warning.png) 与 [访达收藏探针](v4-finder-favorites-probe.md)。

## 2. 不适用（用户决定）

| 项目                                         | 状态                                     |
| -------------------------------------------- | ---------------------------------------- |
| `.github/workflows/ci.yml`（PD-210）         | 不适用（用户决定）                       |
| Apple Silicon `.app`/`.dmg` 打包（PD-211）   | 不适用；当前 bundle 关闭，只产出裸二进制 |
| npm/Tauri/Cargo 版本同步与标签校验（PD-212） | 不适用（用户决定）                       |
| GitHub Pre-release 自动发布（PD-213）        | 不适用（用户决定）                       |
| Release 安装、校验与失败重试文档（PD-214）   | 不适用（用户决定）                       |
| v3 文档切换与标签（PD-220）                  | 不适用（用户决定）                       |

当前版本仍为 `0.1.0`；v4 是产品迭代名称，不代表已发布 `0.4.0`。没有 GitHub Release、标签、SHA-256 校验文件或公开下载链接。

## 3. 本地测试包（仅本机）

用于只读验证，不是可分发包：

```bash
npm run build
mkdir -p /tmp/PaneDeck.app/Contents/MacOS
cp target/release/panedeck /tmp/PaneDeck.app/Contents/MacOS/PaneDeck
# Info.plist：CFBundleIdentifier=dev.panedeck.desktop，LSMinimumSystemVersion=14.0
codesign --force --deep --sign - /tmp/PaneDeck.app
open -n /tmp/PaneDeck.app
```

注意：

- `LSSharedFileList` 只对经 LaunchServices 启动的 `.app` 暴露用户收藏；直接执行 bundle 内二进制会返回空列表。
- 若运行环境限制 `~/Library/Caches` 写入，WebKit 可能无法创建窗口；本机原生审查使用工作区可写缓存或隔离 HOME 绕过，不能作为发布环境结论。
- `.app` 为 ad-hoc 签名，未做 Developer ID 签名与公证；对外分发会触发 Gatekeeper 提示。
- 不包含 `.dmg`、后台更新、自动安装或回滚机制。

## 4. 若未来恢复发布需补齐

1. 在真实 GitHub 仓库实现 PD-210～PD-214 的 CI、ARM runner、打包、版本同步、Pre-release 和安装验证。
2. 记录工作流运行 URL、提交 SHA、runner 架构、产物 SHA-256、下载/安装/启动证据。
3. 处理未测项：外接盘/NAS/第三方云盘、macOS 14～16、访达可见侧栏范围、真实挂载通知、原生打开交互与 VoiceOver/缩放。
4. 更新 README、架构、v4 验收矩阵与版本说明；披露无 Developer ID/公证的限制。
5. 只有全部证据齐全后，才可恢复 PD-340；当前按用户决定不适用，不以本地构建或规划文档代替发布验收。
