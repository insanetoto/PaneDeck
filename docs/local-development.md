# 本地开发与临时目录规则

> 适用于本机调试、测试、构建和探针运行。所有由本项目生成的临时/调试文件必须位于项目目录内。

## 统一位置

```text
<project>/.tmp/
├── dev-home/       # npm run dev 的隔离 HOME / WebKit / Application Support
├── dev-tmp/        # npm run dev 的 TMPDIR
├── build-tmp/      # npm run build 的 TMPDIR
├── test-tmp/       # Rust/端到端测试的 TMPDIR
├── bench/          # 基准测试 fixture
├── release/        # 本地交付物（DMG、ZIP、校验文件）
└── finder-favorites-probe.txt
```

`.tmp/` 已在 `.gitignore` 中忽略。

## 命令

| 命令                                      | 行为                                                                       |
| ----------------------------------------- | -------------------------------------------------------------------------- |
| `npm run dev`                             | `CFFIXED_USER_HOME=.tmp/dev-home`、`TMPDIR=.tmp/dev-tmp`，隔离真实用户目录 |
| `npm run build`                           | `TMPDIR=.tmp/build-tmp`                                                    |
| `npm run test:rust`、`npm run check:rust` | `TMPDIR=.tmp/test-tmp`                                                     |
| `npm run test:e2e*`                       | `TMPDIR=.tmp/test-tmp`                                                     |
| `npm run benchmark:rust`                  | `TMPDIR=.tmp/bench-tmp`，fixture 位于 `.tmp/bench`                         |
| 探针                                      | 默认写入 `.tmp/finder-favorites-probe.txt`                                 |

直接运行 `cargo test` 时请手动指定：

```bash
TMPDIR="$PWD/.tmp/test-tmp" cargo test --workspace --locked
```

## 不得创建的位置

- `/tmp`、`/var/tmp` 等系统临时目录；
- `/Users/<user>/...` 根目录或用户文档目录下的调试产物；
- `~/Documents` 等个人目录中的临时 fixture；
- 项目外的截图、探针输出、`.app` 包装或 DMG 构建产物。

## 说明

- `npm run dev` 默认使用隔离 HOME，因此访达收藏、iCloud 等真实系统数据不会出现在调试会话中；这是为了满足目录隔离要求。
- 已安装应用运行时的 Application Support / WebKit 数据属于用户应用数据，不是临时目录；由应用内卸载功能清理。
- 发布 DMG 在 `.tmp/release`；GitHub Release 已单独发布。
