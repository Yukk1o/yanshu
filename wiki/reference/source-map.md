# 源码地图

这页把语言概念映射到当前 Rust workspace。先读 `.ail` 示例理解语言，再进入实现 crate；不要从 HTTP server 倒着猜语言语义。

## 推荐阅读顺序

1. [discount/v2.ail](/source/examples/discount/v2.ail.txt)：最小纯函数程序；
2. [service.ail](/source/examples/tasks/service.ail.txt)：Schema、route、capability 与事务 handler；
3. [ail-syntax AST](/source/rust/crates/ail-syntax/src/ast.rs.txt)：Program / Expression / Schema 的数据结构；
4. [ail-syntax Reader](/source/rust/crates/ail-syntax/src/reader.rs.txt) 与 [Parser](/source/rust/crates/ail-syntax/src/parser.rs.txt)：源码怎样成为 AST；
5. [ail-runtime](/source/rust/crates/ail-runtime/src/lib.rs.txt)：解释器、primitive 与 Library Backend；
6. [ail-service](/source/rust/crates/ail-service/src/lib.rs.txt)：route、capability 和事务；
7. [ail-store](/source/rust/crates/ail-store/src/lib.rs.txt)：候选、active 与回滚；
8. [ail-provider](/source/rust/crates/ail-provider/src/lib.rs.txt)：LLM 只能怎样提出候选；
9. [ail-http](/source/rust/crates/ail-http/src/lib.rs.txt)：请求身份、版本固定与观测；
10. [ail-rollout](/source/rust/crates/ail-rollout/src/lib.rs.txt)：隔离影子采样、比较与观测；
11. [ail-ops](/source/rust/crates/ail-ops/src/lib.rs.txt)：服务锁、备份校验与恢复。

## 语言前端

| 文件 | 看什么 |
| --- | --- |
| [syntax/lib.rs](/source/rust/crates/ail-syntax/src/lib.rs.txt) | `load_program_source` 的 Reader → Parser 入口 |
| [reader.rs](/source/rust/crates/ail-syntax/src/reader.rs.txt) | UTF-8 token、datum、节点数和嵌套限制、BigInt |
| [ast.rs](/source/rust/crates/ail-syntax/src/ast.rs.txt) | `Program`、`ExpressionKind`、`SchemaKind`、source span、inspect JSON |
| [parser.rs](/source/rust/crates/ail-syntax/src/parser.rs.txt) | 顶层 form、命名、capability、library、Schema、route 和 expression 校验 |
| [diagnostic/lib.rs](/source/rust/crates/ail-diagnostic/src/lib.rs.txt) | 公共 code/message/details 与私有 span |

Go/Rust 读者最值得先看的类型是 `Program`、`ExpressionKind`、`SchemaKind` 和 `Diagnostic`。它们定义语言可表示什么，比某个 CLI 命令更接近语义核心。

## 值与执行

| 文件 | 看什么 |
| --- | --- |
| [runtime/value.rs](/source/rust/crates/ail-runtime/src/value.rs.txt) | Nil、BigInt、List、Map、Result、Closure 与 JSON codec |
| [runtime/budget.rs](/source/rust/crates/ail-runtime/src/budget.rs.txt) | fuel 与调用深度 |
| [runtime/schema.rs](/source/rust/crates/ail-runtime/src/schema.rs.txt) | 默认值、issue 顺序、JSON Pointer 和上限 |
| [runtime/lib.rs](/source/rust/crates/ail-runtime/src/lib.rs.txt) | `execute_export`、环境、闭包、primitive、capability 与 `text@1` |

`Value::Int` 使用 `num_bigint::BigInt`；任何 FFI 或 backend 都不能在没有版本化语言变更时把它收窄成 `i64`。

## Web DSL 与事务

| 文件 | 看什么 |
| --- | --- |
| [service/lib.rs](/source/rust/crates/ail-service/src/lib.rs.txt) | method/path 匹配、request Map、response 校验、capability trait、事务 KV |
| [service.ail](/source/examples/tasks/service.ail.txt) | 真实 Schema、route 与五个 CRUD handler |
| [scenarios.json](/source/examples/tasks/scenarios.json.txt) | 11 个有状态业务契约 |

一次请求的语义顺序：固定 Program → 匹配 route → 创建 working copy → 调用 export → 验证 response → commit 或 discard。

## 版本与 AI 演化

| 文件 | 看什么 |
| --- | --- |
| [store/lib.rs](/source/rust/crates/ail-store/src/lib.rs.txt) | SHA-256、不可变源码校验、metadata、active、events、锁和原子写 |
| [store/scenario.rs](/source/rust/crates/ail-store/src/scenario.rs.txt) | 注册、晋升、重启读取和回滚生命周期 |
| [provider/lib.rs](/source/rust/crates/ail-provider/src/lib.rs.txt) | provider trait、OpenAI/DeepSeek adapter、HTTPS、大小/超时、密钥零化 |
| [cli/main.rs](/source/rust/crates/ail-cli/src/main.rs.txt) | deploy/evolve 如何组合 Parser、suite 和 VersionStore |
| [ops/operations.rs](/source/rust/crates/ail-ops/src/operations.rs.txt) | 离线 backup/verify/restore 的失败关闭流程 |
| [ops/manifest.rs](/source/rust/crates/ail-ops/src/manifest.rs.txt) | snapshot schema、逐文件 SHA-256/大小与路径限制 |
| [ops/lease.rs](/source/rust/crates/ail-ops/src/lease.rs.txt) | server 生命周期 service lock 与维护互斥 |
| [rollout/policy.rs](/source/rust/crates/ail-rollout/src/policy.rs.txt) | 固定候选 hash 与 request ID 确定性采样 |
| [rollout/comparison.rs](/source/rust/crates/ail-rollout/src/comparison.rs.txt) | 只在内存比较状态、handler、错误与内容摘要 |
| [rollout/observation.rs](/source/rust/crates/ail-rollout/src/observation.rs.txt) | 不含请求/响应内容和内容指纹的 JSONL schema |

`evolve_service_with_provider` 是最短的控制流入口：读 active → 测当前版本 → 请求候选 → 解析候选 → 测完整 suite → 注册 → 可选晋升。

运维快照不包含 observations；它校验版本源码、metadata、active、事件序列和可选 KV v1 文档。恢复始终拒绝覆盖既有 code/data 目标。

## Rust 请求安全链

按一个 HTTP 请求的真实顺序阅读：

1. [server/main.rs](/source/rust/crates/ail-server/src/main.rs.txt)：读取参数与可选 token，拒绝非 loopback bind；
2. [http/lib.rs](/source/rust/crates/ail-http/src/lib.rs.txt)：Bearer 摘要与常量时间比较，生成宿主 request ID；
3. 同一文件中的 `LoadedProgram` 路径：读取 active hash、校验内容并固定 Program；
4. header 投影：过滤 `authorization`、`cookie`、`proxy-authorization`、`x-api-key`、`x-request-id`；
5. [service/lib.rs](/source/rust/crates/ail-service/src/lib.rs.txt)：匹配 route、执行事务 handler；
6. 回到 `http/lib.rs`：响应加入 `X-Request-Id`，观测写入固定版本 hash。
7. [http/shadow.rs](/source/rust/crates/ail-http/src/shadow.rs.txt)：有界后台候选读取请求前快照，比较后丢弃副作用。

观测字段白名单是 `schemaVersion/timestampMs/requestId/method/status/durationMs/handler/version/errorCode`。测试使用特殊秘密值确认 path、query、header、body 和内部诊断不会进入 JSONL。

## Library Backend

当前 `text@1` contract、类型检查、fuel 计费和结果归一化都在 [ail-runtime](/source/rust/crates/ail-runtime/src/lib.rs.txt)。Parser 只允许精确 `(text 1)`，示例在 [text.ail](/source/examples/libraries/text.ail.txt)。

外部 crate provider、稳定 FFI、WASM/sidecar backend 尚未实现；路线见[标准库与 Library Backend](/language/standard-library)和[Rust 宿主与生态路线](/development/rust-roadmap)。

## Workspace 与安全策略

| 文件 | 用途 |
| --- | --- |
| [Cargo.toml](/source/Cargo.toml.txt) | workspace 成员、Rust 1.97、依赖版本、`unsafe_code = forbid` |
| [Cargo.lock](/source/Cargo.lock.txt) | 可复现依赖锁 |
| [deny.toml](/source/deny.toml.txt) | advisory、license、source 与 duplicate 策略 |
| [rust-safety-policy.md](/source/docs/rust-safety-policy.md.txt) | 第一方 unsafe 与依赖审计边界 |
| [rust-dependency-audit.md](/source/docs/rust-dependency-audit.md.txt) | 第三方依赖 unsafe 清单与复核记录 |
