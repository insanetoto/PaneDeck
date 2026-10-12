# PaneDeck

PaneDeck 是一个面向 macOS 的双窗格文件整理工具。它希望让移动、复制、重命名、删除与批量整理变得更快，同时通过任务队列和冲突预检降低误操作成本。

项目已完成 MVP：工程质量门、核心模块边界、双语框架、双窗格导航与虚拟列表、外部变化同步、本地位置与收藏、文件操作预检和任务队列，以及安全复制、移动、剪切/粘贴、重命名、新建文件夹、系统废纸篓、统一冲突决策、双窗格拖放、主动诊断导出与崩溃恢复提示。

## 当前产品定义

- 运行环境：macOS 14+，仅支持 Apple Silicon
- 核心技术：Rust 文件系统与任务引擎
- UI：Tauri 2 + React/TypeScript
- 核心体验：双窗格、键盘优先、任务可见、冲突明确
- MVP 边界：本地文件管理，不包含云盘、远程协议、压缩包编辑和插件系统
- 使用方式：本地个人使用，暂不考虑分发与商业化
- 语言：简体中文、英文
- 隐私：无遥测；诊断信息仅由用户主动导出，并移除完整路径

当前 v2 规划见 [docs/product-plan-v2.md](docs/product-plan-v2.md)，任务见 [docs/tasks-v2.md](docs/tasks-v2.md)。v1 [规划](docs/product-plan.md)与[任务](docs/tasks.md)保留为历史安全基线。
v2 已完成 PD-100～PD-102（命令契约、操作来源路由、macOS 菜单栏）、PD-110～PD-113（访达式展开目录树、树选择与外部变化同步）及 PD-120 → PD-103 → PD-121（原生上下文菜单、顶栏和活动标识移除、同窗格/跨窗格拖放）。PD-130～PD-133 已完成持久操作日志、真实阶段事件、底部进度和异常提示，并移除右侧任务抽屉。PD-140～PD-141 已完成自动审计、资源回归和文档切换；2026-10-12 用户反馈测试步骤第 1～7 点已测试完毕，第 8 点中断恢复/VoiceOver、第 9 点实机性能/资源暂缓。随后发现 iCloud 桌面目录移动、复制回原处后删除失败的缺陷，已调整系统废纸篓实现，实际 iCloud 场景仍待复测，不宣称全部实机项目通过。验收证据见 [docs/v2-acceptance.md](docs/v2-acceptance.md)。
多人协作时使用 [docs/collaboration-prompt.md](docs/collaboration-prompt.md) 中的 GitHub 领取约定与可复制提示词。
首次参与开发请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 的完整协作步骤。
品牌图标的设计语义与使用边界见 [branding/README.md](branding/README.md)。
Rust 模块和依赖方向见 [docs/architecture.md](docs/architecture.md)。
当前端到端证据见 [docs/v2-acceptance.md](docs/v2-acceptance.md)，实机待验收项见 [docs/v2-manual-audit.md](docs/v2-manual-audit.md)；[MVP 验收](docs/mvp-acceptance.md)保留为 v1 历史。

## 开发方式

项目不设时间周期。每次选择一个依赖已满足的任务交给 AI 实现，通过该任务的验收标准后再标记完成。

## 本地命令

前置条件：Apple Silicon Mac、macOS 14+、Rust stable（包含 `rustfmt`、`clippy`）、Node.js 22.12+、npm 10 和 Xcode Command Line Tools。当前验证环境为 Rust 1.93.1、Node.js 22.16.0、npm 10.9.2。

```bash
npm install
npm run dev
npm test
npm run test:e2e
npm run check
npm run build
```

- `npm run dev`：启动 Vite 与 Tauri 开发窗口。
- `npm run benchmark`：在系统临时目录生成固定数据，运行前端与 Rust 本地性能基准。
- `npm test`：运行前端与 Rust workspace 测试。
- `npm run test:e2e`：运行 MVP UI 流程和 Rust 临时目录端到端验收。
- `npm run check`：依次检查 Cargo crate 边界、前端格式/lint/类型/测试，以及 Rust fmt/clippy/test。
- `npm run format`：统一格式化前端、配置、Markdown 与 Rust 源码。
- `npm run build`：生成本地 Release 可执行文件，不创建安装包或应用分发包。
