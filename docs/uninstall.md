# PaneDeck 卸载

> 实现：2026-10-26
> 相关代码：[uninstall.rs](../crates/panedeck-app/src/uninstall.rs) · [UninstallDialog.tsx](../src/uninstall/UninstallDialog.tsx) · [app_removal.rs](../crates/panedeck-platform/src/app_removal.rs)

## 1. 使用方式

1. 启动 PaneDeck。
2. 菜单栏 **PaneDeck → 卸载 PaneDeck…**。
3. 检查弹窗列出的应用数据路径、大小和应用包位置。
4. 确认后：
   - 应用数据被永久删除；
   - `PaneDeck.app` 被移到废纸篓；
   - 点击“退出 PaneDeck”结束进程。
5. 如需彻底移除应用，清空废纸篓。
6. 完全磁盘访问等系统权限需在 **系统设置 → 隐私与安全性** 中手动移除。

直接拖拽 `PaneDeck.app` 到废纸篓**不会**删除应用数据；请先使用应用内卸载。

## 2. 删除范围（固定允许列表）

只删除 `~/Library` 下当前应用标识 `dev.panedeck.desktop` 的路径：

| 路径                                                      | 内容                |
| --------------------------------------------------------- | ------------------- |
| `Application Support/dev.panedeck.desktop`                | 操作日志、恢复记录  |
| `Caches/dev.panedeck.desktop`                             | 缓存                |
| `Preferences/dev.panedeck.desktop.plist`                  | 偏好设置            |
| `WebKit/dev.panedeck.desktop`                             | WebView 本地数据    |
| `HTTPStorages/dev.panedeck.desktop`                       | HTTP 存储           |
| `Saved Application State/dev.panedeck.desktop.savedState` | 保存的窗口/应用状态 |
| `Logs/PaneDeck`                                           | 日志（如存在）      |

另外仅移动当前运行的 `.app` 到废纸篓，不永久删除应用包。

## 3. 明确不会删除

- 用户文稿、桌面、下载、照片等个人文件；
- iCloud Drive、外接盘、网络卷；
- 废纸篓中的原有内容；
- 其他应用或旧标识（如 `dev.panedeck.review`、`panedeck`）的数据；
- Finder 共享收藏、系统偏好、TCC 权限记录。

## 4. 安全约束

- 路径来自固定允许列表，并再次校验位于 `~/Library` 下；
- 不跟随符号链接；数据路径本身是符号链接时只删除链接；
- 应用包必须是扩展名 `.app`、不是符号链接，且 `Info.plist` 中 `CFBundleIdentifier` 为 `dev.panedeck.desktop`；
- 应用包通过 `NSFileManager.trashItemAtURL` 移到废纸篓，可恢复；
- 应用包移动失败时，**不删除任何应用数据**；
- 数据删除失败会逐项报告，不会静默成功；
- 不执行 shell、不拼接命令、不删除任意路径。

## 5. 已知限制

- 完全磁盘访问等 TCC 权限不能由应用删除，需要用户手动处理；
- 应用被直接拖入废纸篓时不会触发数据清理；
- 旧版本标识的数据不在当前允许列表内，避免误删；
- 未打包的开发二进制没有 `.app`，此时只能删除应用数据；
- 应用包进入废纸篓后，需用户清空废纸篓才算永久移除。

## 6. 开发验证

```bash
cargo test -p panedeck-app uninstall::tests
npm run check:frontend
npm run build
```

测试覆盖：

- 只删除允许列表内的应用数据；
- 用户目录文件保持不变；
- 空操作和非法应用包被拒绝；
- 卸载确认、部分失败提示与退出按钮。
