# PaneDeck v4 实施基线（PD-300）

> 记录日期：2026-10-23
> 任务：[tasks-v4.md](tasks-v4.md) · 方案：[product-plan-v4.md](product-plan-v4.md) · 验收：[v4-acceptance.md](v4-acceptance.md)
> 交付方式：仅本地改动；未提交、未推送、未修改 codex/Cargo/工具配置

## 1. 工作区与工具链

- 分支：`codex/fix-operation-completion`，HEAD `62275d5`；`main` 为 `9ee850d`。
- 开始 PD-300 时工作区已有 v3 未提交改动：10 个修改文件（+158/−38），以及未跟踪的 v3/v4 文档与截图。本基线**不覆盖**这些改动，v4 改动叠加在现有工作区。
- 本机：Node 24.21.0、npm 11.19.0、Rust/Cargo 1.99.0 stable。`package.json` 声明 Node 22 / npm 10，当前环境超出声明范围，未修改 `engines`。
- 基线质量门：`npm run typecheck` 通过；`npx vitest run` 17 个文件 / 81 项通过。Rust 测试本机因 crates.io 网络超时未能复跑；[v3 验收](v3-acceptance.md)记录的历史结果为 120 项常规测试通过、2 项真实废纸篓测试默认忽略。

## 2. 改动前的字体、行高与列宽

| 项目            | 改动前                                                                | v4 最终值                       |
| --------------- | --------------------------------------------------------------------- | ------------------------------- |
| 文件名          | 13px                                                                  | **14px**                        |
| 元数据 / 表头   | 12px                                                                  | 12px（不变）                    |
| 侧栏条目        | 12px                                                                  | **13px**                        |
| 侧栏分组标题    | 10px                                                                  | **11px**                        |
| 文件行高        | 26px                                                                  | **28px**                        |
| 表头高度        | 28px                                                                  | 28px                            |
| 文件图标 / 缩进 | 16px / 16px                                                           | 16px / 16px（候选 16～18 未改） |
| 列宽            | 名称 `minmax(180px, 1fr)`、大小 92、种类 92、修改 150；表格最小 520px | 四列统一 CSS 变量模型           |

## 3. 列宽契约

### 默认值与范围

| 列       | 默认                         | 最小 | 最大 |
| -------- | ---------------------------- | ---- | ---- |
| 名称     | 自动（占剩余宽度，最小 180） | 180  | 720  |
| 大小     | 92                           | 64   | 320  |
| 种类     | 92                           | 80   | 320  |
| 修改日期 | 150                          | 120  | 400  |

- 表格最小宽度 = `max(520, 四列宽度之和)`；自动名称列按最小 180 计入。
- 名称列保持 `minmax(180px, 1fr)`；用户首次拖动名称后转为固定像素，双击恢复自动。
- 不包含列重排、隐藏列、扫描后自动适宽。

### 交互

- 每个表头右侧 8px 命中区，`role="separator"`、`aria-orientation="vertical"`，带列名、最小/最大与当前值。
- 指针捕获；释放提交，`pointercancel` / 失去捕获恢复拖动前的值；越界按上下限钳制。
- 双击恢复该列默认；键盘 `←` / `→` 每次 8px，`Shift` 加速为 24px。
- 拖动不触发排序、行选择或文件拖放；表头与行共享同一列宽模型。
- 横向滚动由虚拟列表视口承担，表头按 `scrollLeft` 平移保持对齐；空目录仍渲染表头。

### 持久化

- 键：`panedeck.column-widths.v1`，仅本地 `localStorage`。
- 结构：`{ version: 1, panes: { left: ColumnWidths, right: ColumnWidths } }`。
- `name` 为数字或 `null`（自动）；左右窗格独立，写入一个窗格保留另一个。
- 版本不符、JSON 损坏、字段非数字时回退；越界值钳制到最近边界；读写异常不阻止浏览。

## 4. 侧栏选择/打开契约（PD-312，尚未实施）

- 目标语义：单击只选择，不改变主文件区、路径或历史；双击 / `Enter` / `Command-Down` 才进入。
- 当前基线：侧栏位置与收藏均单击直接导航，尚无“选中”状态；该差异保留至 PD-312，本阶段未改动。
- 后续：PD-312 已改为单击选择、双击/Enter/Command-Down 进入；实施证据见 [v4-acceptance.md](v4-acceptance.md) 的 B/C 组记录。

## 5. 参考与模块盘点

- 用户此前参考图未随本轮消息提供；v3 浏览器截图只能作前后对比，不能冒充访达或用户参考图。
- 主要模块：[VirtualFileList.tsx](../src/browser/VirtualFileList.tsx)、[App.tsx](../src/App.tsx)、[App.css](../src/App.css)、[tokens.css](../src/styles/tokens.css)、[columnWidths.ts](../src/browser/columnWidths.ts)、中英文消息与相关测试。
- 未取得真实访达截图、原生 WebView、200% 系统缩放或 VoiceOver 证据；相关验收保持未测。
