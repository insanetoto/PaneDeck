# PaneDeck 协作开发指南

PaneDeck 采用“一个 GitHub Issue、一个负责人、一个分支、一个 Pull Request”的开发方式。每次只完成一个任务，任务边界与验收标准以对应 Issue 为准。

## 1. 仓库所有者完成一次性设置

1. 在 GitHub 仓库的 `Settings → Collaborators` 中邀请同事；私有仓库必须先完成这一步。
2. 将 `docs/tasks.md` 中准备开发的任务分别建立为 GitHub Issue，标题格式为 `[PD-XXX] 任务名称`。
3. 把任务的依赖、目标、交付和验收原文复制到 Issue 正文。
4. 如果当前 GitHub 套餐支持，建议为 `main` 设置分支保护：必须通过 Pull Request 合并、至少一人评审、禁止 force push、禁止删除分支。建立自动检查后，再要求检查通过才能合并。

GitHub Issue 的 Assignee 是任务领取状态的唯一事实来源。`docs/tasks.md` 用于维护产品任务目录，不用于多人同时抢占任务。

## 2. 同事准备本地环境

需要：Apple Silicon Mac、macOS 14+、Git、Xcode Command Line Tools、Rust stable 和 Node.js 22+。

```bash
xcode-select --install
rustup toolchain install stable
rustup default stable
node --version
rustc --version
```

如果 GitHub 仓库是私有的，还需要先接受协作者邀请，并用个人访问令牌、SSH Key 或 GitHub CLI 完成 GitHub 身份验证。

## 3. 首次克隆与验证基线

```bash
git clone https://github.com/insanetoto/PaneDeck.git
cd PaneDeck
npm install
npm test
npm run build
```

只有基线测试和构建通过后再领取任务。若基线失败，先在 GitHub 建立或反馈独立问题，不要把无关修复混进准备领取的任务。

## 4. 领取任务

1. 打开仓库的 `Issues` 页面。
2. 选择一个没有 Assignee、并且所有依赖任务均已合并的 Issue。
3. 将自己设置为 Assignee，并留言：`领取此任务，计划在 task/pd-xxx-short-name 分支开发。`
4. 如果无法自行设置 Assignee，请让仓库管理员分配后再开始。
5. 不要领取已有负责人的 Issue；需要交接时先由原负责人取消分配并留下交接说明。

## 5. 从最新 main 创建任务分支

下面以 `PD-002` 为例，实际名称使用对应任务编号与简短英文描述：

```bash
git switch main
git pull --ff-only origin main
git switch -c task/pd-002-quality-gate
```

禁止直接在 `main` 上开发。一个任务分支只解决一个 Issue。

## 6. 使用 AI 完成任务

打开 `docs/collaboration-prompt.md`，复制其中的完整提示词，并替换：

- 任务编号
- GitHub Issue 地址
- 当前任务分支
- 交付方式
- 明确的补充要求

建议第一次让 AI 只保留本地改动、不直接推送；开发者检查结果后再授权提交或自行提交。AI 必须先检查依赖、现有代码和基线测试，只实现当前 Issue，不提前实现后续任务。

## 7. 开发者本地验收

至少运行：

```bash
npm test
npm run build
git status --short
git diff --check
git diff
```

同时逐条核对 Issue 的验收标准。涉及 UI 时人工检查简体中文、英文、浅色和深色主题；涉及文件操作时确认自动化测试只使用测试创建并持有的临时目录。

验收失败时不得把 `docs/tasks.md` 中的任务标记为“已完成”。

## 8. 提交与推送任务分支

只暂存当前任务相关文件：

```bash
git add <本任务相关文件>
git status --short
git commit -m "feat(pd-002): establish local quality gate"
git push -u origin task/pd-002-quality-gate
```

提交类型可按实际情况使用 `feat`、`fix`、`test`、`docs` 或 `refactor`。提交信息必须包含任务编号，避免使用含义不明的 `update`、`changes`。

## 9. 创建 Pull Request

在 GitHub 上从任务分支向 `main` 创建 Pull Request。PR 正文至少包含：

```markdown
Closes #<Issue 编号>

## 完成内容
- ...

## 明确未包含
- ...

## 验收证据
- `npm test`：通过
- `npm run build`：通过

## 风险与人工检查
- ...
```

UI 改动附中英文以及必要的浅色/深色截图。不要自行合并自己的 PR；等待另一位同事检查任务边界、实现和验收证据。

## 10. 处理评审意见与主分支更新

修改后重新运行验收，再提交并推送到同一分支：

```bash
git add <修改文件>
git commit -m "fix(pd-002): address review feedback"
git push
```

如果等待评审期间 `main` 已更新：

```bash
git fetch origin
git merge origin/main
npm test
npm run build
git push
```

解决冲突时必须理解双方改动，不得简单覆盖他人的实现。

## 11. 合并后的收尾

PR 通过评审和检查后，由维护者合并。开发者随后更新本地仓库并删除已经合并的本地分支：

```bash
git switch main
git pull --ff-only origin main
git branch -d task/pd-002-quality-gate
```

若远程分支未由 GitHub 自动删除，可在确认 PR 已合并后从 GitHub 页面删除。不要使用强制删除来掩盖尚未合并的提交。
