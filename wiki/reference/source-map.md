# 源码地图

Wiki 构建前会执行 `scripts/sync-source.mjs`，把明确白名单中的仓库文件原样复制到 `public/source/`。因此下面链接是**本次 Wiki 构建时的真实源码快照**，不是手工摘录；密钥、`.runtime/` 和工具链不会发布。

## 从哪里开始读

建议按以下顺序，不需要先掌握 Racket：

1. [ast.rkt](/source/src/ast.rkt.txt)：先看数据结构，等价于 Go struct / Rust enum；
2. [runtime.rkt](/source/src/runtime.rkt.txt)：看 `evaluate` 和 primitive；
3. [service.rkt](/source/src/service.rkt.txt)：看一次业务请求怎样调用 export；
4. [kv-store.rkt](/source/src/kv-store.rkt.txt)：看事务提交边界；
5. [evolution-loop.rkt](/source/src/evolution-loop.rkt.txt)：看 AI 候选怎样进入测试和版本库。

如果要看正在迁移的 Rust 宿主，按 `ail-diagnostic → ail-syntax → ail-runtime →
ail-conformance → ail-store → ail-service → ail-cli` 阅读；这些 crate 与 Racket oracle
共享同一份 fixtures。

## Rust v0.5 宿主

| 文件 | 负责什么 |
| --- | --- |
| [diagnostic/lib.rs](/source/rust/crates/ail-diagnostic/src/lib.rs.txt) | 稳定诊断和源码位置 |
| [syntax/reader.rs](/source/rust/crates/ail-syntax/src/reader.rs.txt) | 无 unsafe 的有界 Reader |
| [syntax/parser.rs](/source/rust/crates/ail-syntax/src/parser.rs.txt) | Program/Schema/Expr 静态检查 |
| [runtime/lib.rs](/source/rust/crates/ail-runtime/src/lib.rs.txt) | arena 环境、闭包、求值、primitive、Library Backend |
| [runtime/schema.rs](/source/rust/crates/ail-runtime/src/schema.rs.txt) | Schema normalization 和 issues |
| [conformance/lib.rs](/source/rust/crates/ail-conformance/src/lib.rs.txt) | 共享 manifest runner 和 portable codec |
| [store/lib.rs](/source/rust/crates/ail-store/src/lib.rs.txt) | 原子文件替换、版本内容寻址、晋升与回滚 |
| [store/scenario.rs](/source/rust/crates/ail-store/src/scenario.rs.txt) | Racket/Rust 版本库生命周期差分 |
| [service/lib.rs](/source/rust/crates/ail-service/src/lib.rs.txt) | capability trait、路由、事务/文件 KV 与场景 runner |
| [http/lib.rs](/source/rust/crates/ail-http/src/lib.rs.txt) | Axum/Tokio 请求边界、活动版本 loader 与 TCP 测试 |
| [provider/lib.rs](/source/rust/crates/ail-provider/src/lib.rs.txt) | OpenAI/DeepSeek 请求响应、HTTPS 边界与凭据清零 |
| [server/main.rs](/source/rust/crates/ail-server/src/main.rs.txt) | Rust HTTP 进程入口、监听与优雅关闭 |
| [cli/main.rs](/source/rust/crates/ail-cli/src/main.rs.txt) | Rust check / inspect / conformance CLI |

## 语言内核

| 文件 | 负责什么 | Rust 迁移目标 |
| --- | --- | --- |
| [reader.rkt](/source/src/reader.rkt.txt) | 安全读取单个 S 表达式，限制节点/深度 | `reader` crate/module |
| [ast.rkt](/source/src/ast.rkt.txt) | Program、Expr、Route、Schema 数据结构 | `ast::{Program, Expr, Schema}` |
| [parser.rkt](/source/src/parser.rkt.txt) | 语法、唯一性、能力和路由静态检查 | `parser::parse_program` |
| [runtime.rkt](/source/src/runtime.rkt.txt) | 环境、闭包、求值、fuel、primitive | `runtime::{Vm, Value, Diagnostic}` |
| [schema.rkt](/source/src/schema.rkt.txt) | 有边界的递归业务校验 | `schema::validate` |
| [library-contract.rkt](/source/src/library-contract.rkt.txt) | 标准库名称、版本、类型和 fuel 契约 | `ail_library::contract` |
| [library-backend.rkt](/source/src/library-backend.rkt.txt) | 可替换的 Racket 参考实现 | Rust crate adapters |
| [error.rkt](/source/src/error.rkt.txt) | 稳定诊断 code/message/details | `enum DiagnosticCode` + struct |

## Web 数据面

| 文件 | 负责什么 |
| --- | --- |
| [http-server.rkt](/source/src/http-server.rkt.txt) | TCP/HTTP/JSON、限制、超时、静态测试页 |
| [service.rkt](/source/src/service.rkt.txt) | 路由、guest request、handler 执行、response 验证 |
| [kv-store.rkt](/source/src/kv-store.rkt.txt) | 内存/文件 KV、请求事务、原子文件替换 |
| [service-test-suite.rkt](/source/src/service-test-suite.rkt.txt) | 固定时钟的有状态业务场景 |
| [service-deployment.rkt](/source/src/service-deployment.rkt.txt) | 测试门禁、活动 loader、服务演化组合 |

## AI 控制面

| 文件 | 负责什么 |
| --- | --- |
| [evolver.rkt](/source/src/evolver.rkt.txt) | provider 接口、prompt、OpenAI/DeepSeek 响应验证 |
| [http-json.rkt](/source/src/http-json.rkt.txt) | 有超时和长度限制的 HTTPS JSON POST |
| [evolution-loop.rkt](/source/src/evolution-loop.rkt.txt) | 当前报告 → 候选 → 测试 → 注册 → 可选晋升 |
| [version-store.rkt](/source/src/version-store.rkt.txt) | SHA-256 版本、metadata、active、events、rollback |
| [version-store-suite.rkt](/source/src/version-store-suite.rkt.txt) | Racket 侧版本库生命周期差分 runner |
| [test-suite.rkt](/source/src/test-suite.rkt.txt) | 纯函数 JSON 回归 suite |

## 可运行案例

- [任务 CRUD 程序](/source/examples/tasks/service.ail.txt)
- [任务 11 场景](/source/examples/tasks/scenarios.json.txt)
- [有缺陷折扣 v1](/source/examples/discount/v1.ail.txt)
- [修复后折扣 v2](/source/examples/discount/v2.ail.txt)
- [折扣测试](/source/examples/discount/tests.json.txt)
- [text@1 Library Backend 示例](/source/examples/libraries/text.ail.txt)
- [text@1 示例测试](/source/examples/libraries/tests.json.txt)
- [完整宿主测试](/source/tests/all.rkt.txt)

## 原始规格

- [v0.1 语言规格](/source/docs/spec-v0.1.md.txt)
- [v0.2 Web 后端规格](/source/docs/web-backend-v0.2.md.txt)
- [v0.3 业务 Schema 规格](/source/docs/business-backend-v0.3.md.txt)
- [v0.4 Library Backend 规格](/source/docs/library-backend-v0.4.md.txt)
- [Rust Host v0.5 checkpoint](/source/docs/rust-host-v0.5.md.txt)
- [架构与可移植边界](/source/docs/design.md.txt)
- [Live provider 说明](/source/docs/live-provider.md.txt)

::: info 为什么不是 GitHub 链接？
当前仓库没有配置 remote URL。使用构建时白名单快照既保证链接可点击，也避免编造一个不存在的远程仓库。以后配置正式 remote 后，可以把这些链接切换到带 commit hash 的永久链接。
:::
