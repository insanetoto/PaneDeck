# v4 系统位置只读探针（PD-310）

> 记录日期：2026-10-23 · 机器：macOS 27.2 (26B118) / arm64
> 方案：[product-plan-v4.md](product-plan-v4.md) · 任务：[tasks-v4.md](tasks-v4.md) · 验收：[v4-acceptance.md](v4-acceptance.md)
> 原始数据：[system-locations-probe.json](artifacts/v4/system-locations-probe.json)

## 1. 探针与结论

使用隔离探针读取入口元数据，未枚举用户文件、未下载云文件、未挂载卷、未写入任何位置：

- 标准目录：`Home`、`Desktop`、`Downloads`、`Documents` 均存在且为目录。
- iCloud：`~/Library/Mobile Documents/com~apple~CloudDocs` 存在、是目录、是 ubiquitous item，状态 current；没有把应用容器当作云盘根目录。
- `~/Library/CloudStorage`：目录存在，但本机没有第三方 File Provider 入口，计数 0。
- 挂载卷：Foundation `mountedVolumeURLs...` 返回 1 个本地根卷（Macintosh HD，本地、内置、可浏览、root、带 UUID）；没有外接盘或网络卷样本。
- 未取得 macOS 14/15/16、外接盘、NAS、第三方云盘账户的样本，均保留未测。

实现探针：[locations_probe.rs](../crates/panedeck-platform/examples/locations_probe.rs)。运行：

```bash
cargo run -p panedeck-platform --example locations_probe
```

## 2. 数据契约

`LocalLocationsDto`：

```text
locations[]: { id, kind, source, name, path, available, isLocal, isRemovable }
warnings[]: volumes | iCloud | cloudStorage | timeout | refreshFailed
```

- `kind`：`home`、`desktop`、`downloads`、`documents`、`iCloudDrive`、`cloudStorage`、`localVolume`、`externalVolume`、`networkVolume`。
- `source`：`standard`、`iCloud`、`fileProvider`、`volume`。
- `id`：标准目录固定字符串；卷优先 `volume:<UUID>`，缺失 UUID 时用路径；云入口 `cloud:<CloudStorage 目录名>`。
- `available` 只表示入口当前可读为目录，不是写权限。
- 警告只影响对应来源；其他入口仍返回。

## 3. 后台、超时与缓存

- Tauri 命令为异步，平台发现放到阻塞线程；单次最多等待 3 秒。
- 缓存 30 秒；显式刷新跳过缓存。单飞标志保证同时最多一个探针线程。
- 超时返回上次快照并加 `timeout`；后台线程完成后仍会更新缓存。
- 卷、iCloud、CloudStorage 分别局部失败；不会清空其他来源。
- 网络元数据查询不会占用 WebView 主线程。

## 4. 交互契约

- 侧栏“收藏”包含常用目录、访达收藏与应用收藏；“位置”包含 iCloud、云盘入口、本地/外接/网络卷。
- 单击只选择，不改变路径、不读目录；双击、`Enter`、`Command-Down` 才进入。
- 打开仍走现有 `open_directory` 路径解析与会话，不新增写能力。
- 显式刷新按钮、窗口激活节流刷新、原生挂载/卸载通知均只刷新入口，不自动跳转。

## 5. 未测与限制

- 没有外接盘、NAS、第三方云盘账户；相应分类与局部失败未实机覆盖。
- macOS 14+ 的系统 API 兼容性只在 27.2 验证。
- 中文系统语言下的本地化名称未验证；当前机器名称来自 Foundation 本地化资源值。
- 真实 Tauri WebView 的网络卷阻塞表现未测量；超时路径由自动测试覆盖。
