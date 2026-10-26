# PaneDeck v4 验收清单

> 2026-10-26：追加目录单击进入、Finder 原生文件图标与应用内安全卸载（V4-16/17/18）。
> 2026-10-23：PD-300～PD-322 已完成；C 采用用户选择的“隐藏共享项”范围。PD-330 已完成，原生打开/外接/云盘明确待验收；PD-340 按用户决定不适用。V4-01～V4-13 为部分通过。原生 WebView 交互、外接盘/云盘、macOS 14～16、重启恢复与 200% 缩放仍未测；NAS 与 GitHub 发布链路按用户决定不测/不做。不继承 v3 通过记录作为 v4 结果。
> [方案](product-plan-v4.md) · [任务](tasks-v4.md) · [基线](v4-baseline.md) · [交付状态](v4-release.md)

实施后记录提交/工作区、工具链、macOS、测试方式、结果与证据链接。结果使用通过/失败/未测/不适用；不适用需说明原因，不能计为通过。

| 编号  | 步骤                                                       | 预期                                     | 结果                                                    |
| ----- | ---------------------------------------------------------- | ---------------------------------------- | ------------------------------------------------------- |
| V4-01 | 拖四列边界，移出窗口释放，取消拖动                         | 上下限有效，不排序/选择，结束后停止调整  | 部分通过（自动）                                        |
| V4-02 | 左右设不同宽度，切空/非空目录、刷新、伸缩、重启            | 独立恢复，损坏设置回退                   | 部分通过（自动）                                        |
| V4-03 | 拉宽列后横向滚动，10,000 行滚首尾、展开深层                | 表头/行对齐，DOM 有界，无错位            | 部分通过（浏览器几何 + 自动虚拟化）                     |
| V4-04 | 键盘调整、双击恢复、点击文字排序                           | 值/范围可访问，不误选树，仅恢复单列      | 部分通过（浏览器 + 自动）                               |
| V4-05 | 对照访达/参考图，中英文、浅深主题、800×520、200%           | 字体更易读，记录最终数值，必要操作可达   | 部分通过（数值与深色图）                                |
| V4-06 | 启动/刷新位置，记录发现与目录读取调用                      | 只获取入口，无子目录读取/下载/挂载       | 部分通过（自动 + 探针）                                 |
| V4-07 | 单击、双击、键盘打开 iCloud/卷/挂载 NAS/云盘               | 单击选择，确认才读取，正确窗格且一次打开 | 部分通过（原生入口 + 浏览器 + 自动；iCloud/卷打开未测） |
| V4-08 | 无云账号/客户端/挂载环境，来源失败、查询超时               | 不伪造入口，局部失败不冻 UI，可恢复      | 部分通过（自动；无真实云/卷样本）                       |
| V4-09 | 测试卷挂载/卸载，快速开 A 再 B，等待旧请求                 | 入口更新，无自动跳转，无旧结果污染       | 部分通过（自动 + 通知注册；NAS/外接不测）               |
| V4-10 | 对照访达自定义目录收藏，增删后刷新                         | 名称/顺序正确，变化可见，缺项有原因      | 部分通过（用户接受范围：隐藏共享项 + 警告）             |
| V4-11 | 收藏跨来源同路径、同名不同路径，删除应用收藏               | 去重正确，不影响真实目录/系统收藏        | 部分通过（去重通过；共享项隐藏）                        |
| V4-12 | API 不可用、失效书签、权限、虚拟项、离线收藏               | 状态明确，保留有效快照和应用记录，不挂载 | 部分通过（自动；TCC/失效未测）                          |
| V4-13 | 测试目录回归树、右键、复制/移动/拖放/子目录/冲突/日志/进度 | 既有行为、安全与脱敏保持                 | 部分通过（8 项自动集成；原生右键/拖放未测）             |
| V4-14 | 完整质量门、相关集成、构建                                 | 记录真实环境/命令，必要检查通过          | 部分通过（115 前端 / 139 Rust / 构建）                  |
| V4-15 | 复用 v3 流水线，发布/下载 ZIP/DMG、校验、安装              | Apple Silicon 测试版可追溯，安装证据齐全 | 不适用（用户决定不做 GitHub 发布链路）                  |
| V4-16 | 单击桌面/下载/文稿；iCloud/卷保持双击/Enter                | 仅这三个标准收藏单击显示内容             | 部分通过（自动；原生待人工）                            |
| V4-17 | PDF/Word/Excel 图标对照访达                                | 类型图标一致，不阻塞列表                 | 部分通过（原生图标数据 + 前端渲染自动；原生视觉待人工） |
| V4-18 | 应用内卸载：预览、删除应用数据、应用移废纸篓               | 只删应用数据，不删用户文件，失败可恢复   | 部分通过（自动；未在真实安装包上执行）                  |

系统发现和收藏采用只读验证；文件操作仅测试持有目录；挂载/断开选测试专用卷，避免影响个人传输。

既有暂缓项：v2 中断恢复/VoiceOver、实机 FPS/资源继续暂缓；iCloud 桌面移动→复制回原处→删除两侧的实机复测仍未测。键盘语义检查不等于 VoiceOver 实机验收，虚拟化测试不等于 WebView/FPS 验收。

## A 组实施记录（2026-10-23）

### 范围与文件

- 新增 [columnWidths.ts](../src/browser/columnWidths.ts)：四列默认/上下限、表格最小宽度、`panedeck.column-widths.v1` 按窗格读写与损坏回退。
- [VirtualFileList.tsx](../src/browser/VirtualFileList.tsx)：CSS 变量列宽、8px 分隔线命中区、指针捕获、取消回退、键盘/双击复位、表头随横向滚动平移、空目录仍渲染表头。
- [App.tsx](../src/App.tsx)：空目录改为渲染可调表头，虚拟列表增加 `pane` 参数。
- [App.css](../src/App.css) / [tokens.css](../src/styles/tokens.css)：列宽变量、拖动样式、文件名 14px、侧栏 13px、行高 28px。
- 中英文新增列宽朗读与复位文案；未改变既有命令、文件操作或 Rust 边界。

### 自动验证

- `npm run typecheck`：通过。
- `npm run check:frontend`：通过（Prettier、ESLint、tsc、22 个文件 / 115 项测试）。
- `npm run check:boundaries`：通过；`npm run check:scope`：通过（本机 PATH 无 `rg`，使用 DSH 内置 ripgrep 运行）。
- `npm run build:web`：通过；`npm run build`（Tauri release，`--no-bundle`）通过，生成 `target/release/panedeck`。
- 新增覆盖：列宽上下限、指针拖动/取消、键盘 8/24px、双击单列复位（含指针双击兜底）、名称自动/固定切换、左右窗格独立、横向滚动表头对齐、空目录表头、存储损坏/版本/越界/写入失败、28px 令牌与虚拟行高同步、键盘调整不触发树选择/展开、App 空目录集成。
- Rust：`cargo test --workspace` **139 项通过、2 项真实废纸篓测试忽略**；`cargo clippy --workspace --all-targets -D warnings` 与 `cargo fmt --check` 通过。使用 `--offline` + rsproxy 源参数运行，未写入 Cargo 配置。

### 浏览器几何证据

- 页面：临时 `v4-audit.html` + `v4-audit.tsx`（已删除），本地 Vite，Chrome headless `--headless=new`，1100×1000；仅做只读布局、程序化横向滚动、键盘与双击，不连接真实用户目录。
- 面板：480px 默认列宽、480px 预置宽列（名称 600 / 大小 132）、480px 空目录、700px 无横向溢出的宽窗格。Chrome 自带的系统滚动条占 17px，表头通过 `--table-scrollbar-width` 补偿。
- 结果：11 项检查全部为真——四列在默认、宽列、700px 窗格下宽度差均为 0；横向溢出成立；滚动 180px 后表头/行每列 `left` 差为 0；表头 `translateX(-180px)`；键盘 +8px 后为 140px，双击复位 92px；空目录仍有 4 个分隔线和 1 行表头；空状态可见。
- 证据：[浏览器几何 JSON](artifacts/v4/browser-columns.json)、[深色截图](artifacts/v4/browser-columns.png)。截图为系统深色外观，未覆盖浅色主题；截图只包含列表面板，不含侧栏，侧栏 13px 仅为令牌值。
- Chrome 以 `--no-sandbox` 运行仅因本机文件沙箱阻止 Chrome 自身沙箱初始化；未加载外部页面，不构成产品权限变更。

### 追加需求（2026-10-26）

- 单击目录：仅侧栏标准收藏 Desktop/Downloads/Documents 在单击（`detail <= 1`）时打开；个人文件夹、iCloud Drive、Macintosh HD、应用/访达收藏仍单击选择、双击或 Enter 打开；文件列表恢复单击选择、双击打开。
- Finder 图标：`panedeck-platform::file_icon` 用 `NSWorkspace.iconForFile` 生成 32px PNG data URL，按路径缓存；`read_entry_icon` 通过不透明 `EntryReference` 解析路径；前端 `FileIcon` 按扩展名/目录条目缓存并渲染 16px 图标。PDF/Word/Excel 已由自动测试覆盖，原生视觉仍待人工。
- 原生窗口已重启并显示新构建；见 `docs/artifacts` 与后续人工记录。

### 仍未验证

- 真实 Tauri WebView 的指针拖出窗口释放、10,000 行横向滚动、浅色主题、200% 缩放与系统辅助功能。
- 真实重启后的列宽恢复、WebView 禁止 localStorage 时的行为。
- 访达/参考图对照；当前没有用户参考图或原生截图。
- VoiceOver 对 `separator` 与列宽的朗读。
- V4-13～V4-15 的完整回归、构建与发布链路。

## B/C 组实施记录（2026-10-23）

### 范围

- 平台层新增 [locations.rs](../crates/panedeck-platform/src/locations.rs)、[finder_favorites.rs](../crates/panedeck-platform/src/finder_favorites.rs)、[volume_observer.rs](../crates/panedeck-platform/src/volume_observer.rs)：标准目录、iCloud、CloudStorage、卷元数据、`LSSharedFileList` 只读收藏、`NSWorkspace` 挂载/卸载通知。
- 应用层新增 [locations.rs](../crates/panedeck-app/src/locations.rs)、[finder_favorites.rs](../crates/panedeck-app/src/finder_favorites.rs)：30 秒缓存、3 秒有界等待、单飞刷新、局部警告。
- Tauri 新增异步 `list_local_locations`、`list_finder_favorites` 命令与 `locations-changed` 事件；未扩大 capability。
- 前端新增 [useLocalLocations.ts](../src/browser/useLocalLocations.ts)、[useFinderFavorites.ts](../src/browser/useFinderFavorites.ts)、[useLocationsChanged.ts](../src/browser/useLocationsChanged.ts)、[finderFavoritesApi.ts](../src/browser/finderFavoritesApi.ts)；[favorites.ts](../src/browser/favorites.ts) 负责访达/应用收藏去重合并；侧栏改为“收藏”“位置”，单击选择、双击/Enter/Command-Down 进入。

### 只读探针证据

- [系统位置探针记录](v4-system-locations-probe.md) / [JSON](artifacts/v4/system-locations-probe.json)：macOS 27.2、arm64；4 个标准目录、iCloud 目录、1 个本地根卷；CloudStorage 根存在但 0 个第三方入口；无外接盘/云盘样本；NAS 按用户决定不测。
- [访达收藏探针记录](v4-finder-favorites-probe.md) / [JSON](artifacts/v4/finder-favorites-probe.json)：未打包 CLI 返回 0 项；ad-hoc `.app` 无完全磁盘访问时 `visibilityAvailable=false`、共享项 0（警告）；直接二进制可读取归档，8 项 `visibility=0` 全部隐藏；7 个系统项需用 `kCFURLLocalizedNameKey` 回退名称。
- 探针源：[locations_probe.rs](../crates/panedeck-platform/examples/locations_probe.rs)、[finder_favorites_probe.rs](../crates/panedeck-platform/examples/finder_favorites_probe.rs)。只读元数据；未写访达、未挂载、未下载、未枚举提供商子目录。
- 侧栏浏览器审查（临时页面已删除，Chrome headless 1000×760，模拟点击非原生指针）：150px 侧栏、520/380px 两窗格；单击 `openPath` 0 次、双击 1 次；系统收藏 1 个按钮、应用收藏 2 个按钮；未解析警告可见；页面无横向溢出；2 个超长名称按预期省略。证据：[侧栏 JSON](artifacts/v4/sidebar-layout.json)、[深色侧栏截图](artifacts/v4/sidebar-layout.png)。
- 原生窗口历史截图：[native-window.png](artifacts/v4/native-window.png)（启用可见性过滤前）。Tauri release 二进制包入 ad-hoc `.app`，经 LaunchServices（`open`）启动、真实 HOME，macOS 27.2；中文界面、150px 侧栏、双窗格正常，位置显示 iCloud 云盘、Macintosh HD。该图证明原生窗口可读取系统收藏与位置入口，不证明打开交互、外接盘/云盘或刷新竞态；NAS 按用户决定不测。当前未授予完全磁盘访问时的 UI 见 [native-window-visibility-warning.png](artifacts/v4/native-window-visibility-warning.png)：两个窗格提示需要完全磁盘访问，共享收藏被隐藏。
- 但同机 Finder 可见侧栏只显示 iCloud 云盘、xinz、Macintosh HD、隔空投送、网络、废纸篓；LS 共享列表包含更多（可能被 Finder 设置隐藏）项目，`kLSSharedFileListItemHidden` 无可用值。用户要求与访达可见侧栏一致，但决定不授予完全磁盘访问。最终范围为隐藏全部共享收藏并显示权限警告；不展示访达自定义收藏。只读 `sfl4` 过滤仅保留为有权限时的可选路径，不作为当前验收依据。

### 自动验证

- `npm run check:frontend`：通过（Prettier、ESLint、tsc、**22 个文件 / 115 项**）。
- `cargo test --workspace`：**139 项通过、2 项忽略**；`cargo clippy`/`cargo fmt` 通过。
- `cargo test -p panedeck-app --test mvp_e2e`：**8 项通过**，覆盖浏览/复制/移动/重命名/冲突/队列/拖放预检/日志脱敏；原生右键与拖放交互未测。
- `npm run check:boundaries`、`npm run check:scope`：通过（`rg` 缺失时使用 DSH 内置 ripgrep）。
- `npm run build:web`、`npm run build`：通过；未生成 `.app/.dmg`。
- 新增覆盖：位置 DTO/警告映射、缓存/TTL、超时恢复、单飞、访达合并去重、应用/系统来源、单击不导航、双击/Enter、系统收藏无删除按钮、挂载事件订阅清理、Finder 服务缓存/超时、位置刷新不重置列宽或当前目录。

### 仍未验证

- 外接盘、第三方云盘账户；真实网络卷阻塞与 3 秒超时后的资源回收。NAS 按用户决定不测。
- 真实挂载/卸载通知触发 `locations-changed`（只验证了 observer 注册与前端订阅）。
- macOS 14/15/16、中文系统名称、TCC 拒绝、失效书签、虚拟收藏项。
- 访达可见项展示（用户接受隐藏共享项，明确不在当前范围）；iCloud/卷在原生窗口中的打开与视觉。
- V4-13 原生右键/拖放交互、V4-15 发布链路。

以上未测项不计为通过，保留待验收。
