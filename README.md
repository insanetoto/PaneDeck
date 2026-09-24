# PaneDeck

PaneDeck 是一个面向 macOS 的双窗格文件整理工具。它希望让移动、复制、重命名、删除与批量整理变得更快，同时通过任务队列和冲突预检降低误操作成本。

项目已完成工程初始化、质量门、核心模块边界、双语框架、设计令牌、双窗格导航与虚拟列表、外部变化同步、本地位置与收藏、文件操作预检和任务队列，以及安全复制、移动、重命名、新建文件夹与系统废纸篓核心；统一冲突解析和写操作 UI 接入尚未实现。

## 当前产品定义

- 运行环境：macOS 14+，仅支持 Apple Silicon
- 核心技术：Rust 文件系统与任务引擎
- UI：Tauri 2 + React/TypeScript
- 核心体验：双窗格、键盘优先、任务可见、冲突明确
- MVP 边界：本地文件管理，不包含云盘、远程协议、压缩包编辑和插件系统
- 使用方式：本地个人使用，暂不考虑分发与商业化
- 语言：简体中文、英文
- 隐私：无遥测；诊断信息仅由用户主动导出，并移除完整路径

完整规划见 [docs/product-plan.md](docs/product-plan.md)。
开发任务见 [docs/tasks.md](docs/tasks.md)。
多人协作时使用 [docs/collaboration-prompt.md](docs/collaboration-prompt.md) 中的 GitHub 领取约定与可复制提示词。
首次参与开发请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 的完整协作步骤。
品牌图标的设计语义与使用边界见 [branding/README.md](branding/README.md)。
Rust 模块和依赖方向见 [docs/architecture.md](docs/architecture.md)。

## 开发方式

项目不设时间周期。每次选择一个依赖已满足的任务交给 AI 实现，通过该任务的验收标准后再标记完成。

## 本地命令

前置条件：Apple Silicon Mac、macOS 14+、Rust stable（包含 `rustfmt`、`clippy`）、Node.js 22.12+、npm 10 和 Xcode Command Line Tools。当前验证环境为 Rust 1.93.1、Node.js 22.16.0、npm 10.9.2。

```bash
npm install
npm run dev
npm test
npm run check
npm run build
```

- `npm run dev`：启动 Vite 与 Tauri 开发窗口。
- `npm test`：运行前端与 Rust workspace 测试。
- `npm run check`：依次检查 Cargo crate 边界、前端格式/lint/类型/测试，以及 Rust fmt/clippy/test。
- `npm run format`：统一格式化前端、配置、Markdown 与 Rust 源码。
- `npm run build`：生成本地 Release 可执行文件，不创建安装包或应用分发包。
