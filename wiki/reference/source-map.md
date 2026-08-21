# 源码地图

这页把语言概念映射到当前 Rust workspace。先读 `.yan` 示例理解语言，再进入实现 crate；不要从 HTTP server 倒着猜语言语义。

## 推荐阅读顺序

1. [discount/v2.yan](/source/examples/discount/v2.yan.txt)：最小纯函数程序；
2. [费用审批 service.yan](/source/examples/expenses/service.yan.txt)：v2 条件、集合、enum/union、成本与业务 Result；
3. [多模块费用审批](/source/examples/bundles/expense-approval/app.yan.txt)：v3 imports、data、match 与密封 Bundle；
4. [任务 service.yan](/source/examples/tasks/service.yan.txt)：Schema、route、capability 与事务 handler；
5. [yanshu-syntax AST](/source/rust/crates/yanshu-syntax/src/ast.rs.txt)：Program / Expression / Schema 的数据结构；
6. [yanshu-syntax Reader](/source/rust/crates/yanshu-syntax/src/reader.rs.txt) 与 [Parser](/source/rust/crates/yanshu-syntax/src/parser.rs.txt)：源码怎样成为 AST；
7. [yanshu-format](/source/rust/crates/yanshu-format/src/lib.rs.txt)：保留注释、复核语义和幂等性的规范布局；
8. [yanshu-lsp](/source/rust/crates/yanshu-lsp/src/lib.rs.txt)：编辑器怎样复用诊断、导航和 formatter；
9. [yanshu-runtime](/source/rust/crates/yanshu-runtime/src/lib.rs.txt)：解释器、primitive 与 Library Backend；
10. [yanshu-service](/source/rust/crates/yanshu-service/src/lib.rs.txt)：route、capability 和事务；
11. [yanshu-store](/source/rust/crates/yanshu-store/src/lib.rs.txt)：候选、恢复 journal、active 与事件完整性；
12. [yanshu-provider](/source/rust/crates/yanshu-provider/src/lib.rs.txt)：LLM 只能怎样提出候选；
13. [yanshu-http](/source/rust/crates/yanshu-http/src/lib.rs.txt)：请求身份、版本固定与观测；
14. [yanshu-rollout](/source/rust/crates/yanshu-rollout/src/lib.rs.txt)：隔离影子采样、比较与观测；
15. [yanshu-ops](/source/rust/crates/yanshu-ops/src/lib.rs.txt)：服务锁、备份校验与恢复。

## 语言前端

| 文件 | 看什么 |
| --- | --- |
| [syntax/lib.rs](/source/rust/crates/yanshu-syntax/src/lib.rs.txt) | `load_program_source` 的 Reader → Parser 入口 |
| [reader.rs](/source/rust/crates/yanshu-syntax/src/reader.rs.txt) | UTF-8 token、datum、节点数和嵌套限制、BigInt |
| [ast.rs](/source/rust/crates/yanshu-syntax/src/ast.rs.txt) | `Program`、`ExpressionKind`、`SchemaKind`、source span、inspect JSON |
| [node_id.rs](/source/rust/crates/yanshu-syntax/src/node_id.rs.txt) | 不依赖 source offset 的 `expression-v1` 语义路径 |
| [parser.rs](/source/rust/crates/yanshu-syntax/src/parser.rs.txt) | 顶层 form、命名、capability、library、Schema、route 和 expression 校验 |
| [format/lib.rs](/source/rust/crates/yanshu-format/src/lib.rs.txt) | 公共 options/result、双 Parser 语义复核与幂等编排 |
| [format/cst.rs](/source/rust/crates/yanshu-format/src/cst.rs.txt) | 保留注释但丢弃无意义空白的 bounded concrete tree |
| [format/render.rs](/source/rust/crates/yanshu-format/src/render.rs.txt) | form-aware 布局、行宽、缩进与逐次输出上限 |
| [lsp/protocol.rs](/source/rust/crates/yanshu-lsp/src/protocol.rs.txt) | Content-Length framing、JSON body 与 header/message 上限 |
| [lsp/document.rs](/source/rust/crates/yanshu-lsp/src/document.rs.txt) | 文档快照、UTF-16 range、诊断、hover、词法安全跳转与 TextEdit |
| [lsp/server.rs](/source/rust/crates/yanshu-lsp/src/server.rs.txt) | initialize/shutdown/exit、full sync 与 JSON-RPC method dispatch |
| [diagnostic/lib.rs](/source/rust/crates/yanshu-diagnostic/src/lib.rs.txt) | 公共 code/message/details 与私有 span |

## 模块与 Bundle

| 文件 | 看什么 |
| --- | --- |
| [bundle/manifest.rs](/source/rust/crates/yanshu-bundle/src/manifest.rs.txt) | manifest 精确解析、路径 containment、module/root SHA-256 |
| [bundle/graph.rs](/source/rust/crates/yanshu-bundle/src/graph.rs.txt) | missing import、cycle 与 unreachable 检查 |
| [bundle/linker.rs](/source/rust/crates/yanshu-bundle/src/linker.rs.txt) | private namespace、import export、词法 binding 与入口 alias |
| [runtime/matcher.rs](/source/rust/crates/yanshu-runtime/src/matcher.rs.txt) | fuel 计量的递归 pattern binding |

## 类型、效果与只读审查

| 文件 | 看什么 |
| --- | --- |
| [analysis/types.rs](/source/rust/crates/yanshu-analysis/src/types.rs.txt) | 静态 Type 与机器可读表示 |
| [analysis/infer.rs](/source/rust/crates/yanshu-analysis/src/infer.rs.txt) | signature seed、统一、primitive contract、pattern 类型 |
| [analysis/effects.rs](/source/rust/crates/yanshu-analysis/src/effects.rs.txt) | export 调用闭包、高阶 callback 与 capability 校验 |
| [analysis/review.rs](/source/rust/crates/yanshu-analysis/src/review.rs.txt) | 永久只读的 Rust 风格 semantic projection |

## 编译器与 WASM

| 文件 | 看什么 |
| --- | --- |
| [compiler/bytecode.rs](/source/rust/crates/yanshu-compiler/src/bytecode.rs.txt) | 最小栈式指令、code block 与规范 JSON |
| [compiler/compile.rs](/source/rust/crates/yanshu-compiler/src/compile.rs.txt) | 从已验证 AST 降低控制流、闭包、scope 与 pattern |
| [compiler/verify.rs](/source/rust/crates/yanshu-compiler/src/verify.rs.txt) | jump、stack、scope、return 与规模上限 verifier |
| [compiler/artifact.rs](/source/rust/crates/yanshu-compiler/src/artifact.rs.txt) | Program 指纹、artifact hash、规范 envelope 与同源重验 |
| [compiler/wasm.rs](/source/rust/crates/yanshu-compiler/src/wasm.rs.txt) | 标准 WASM 编码、`yanshu_v1.execute` handle ABI 与 bytecode custom section |
| [runtime/compiled.rs](/source/rust/crates/yanshu-runtime/src/compiled.rs.txt) | 验证后字节码执行循环、词法 scope、闭包、match 与语义 fuel 计费点 |
| [runtime/lib.rs](/source/rust/crates/yanshu-runtime/src/lib.rs.txt) | 解释器和编译 VM 共享的 Value、primitive、Library/capability 与动态 fuel |

编译产物不信任自己携带的声明。加载器以已验证 Program 或 package lock 重新生成规范产物再完整比较；VM 执行前仍运行 verifier。完整契约见 [fuel 字节码与 WASM](/language/bytecode-wasm)。

## 内容寻址包与锁文件

| 文件 | 看什么 |
| --- | --- |
| [package/model.rs](/source/rust/crates/yanshu-package/src/model.rs.txt) | source descriptor、artifact manifest 与 lock 数据模型 |
| [package/parse.rs](/source/rust/crates/yanshu-package/src/parse.rs.txt) | exact-field、排序、边界、hash 和路径格式解析 |
| [package/store.rs](/source/rust/crates/yanshu-package/src/store.rs.txt) | 递归打包、SHA-256 store、闭包重验、链接与 lock 比对 |

Go/Rust 读者最值得先看的类型是 `Program`、`ExpressionKind`、`SchemaKind` 和 `Diagnostic`。它们定义语言可表示什么，比某个 CLI 命令更接近语义核心。

## 值与执行

| 文件 | 看什么 |
| --- | --- |
| [runtime/value.rs](/source/rust/crates/yanshu-runtime/src/value.rs.txt) | Nil、BigInt、List、Map、Result、Closure 与 JSON codec |
| [runtime/budget.rs](/source/rust/crates/yanshu-runtime/src/budget.rs.txt) | fuel 与调用深度 |
| [runtime/schema.rs](/source/rust/crates/yanshu-runtime/src/schema.rs.txt) | 默认值、issue 顺序、JSON Pointer 和上限 |
| [runtime/lib.rs](/source/rust/crates/yanshu-runtime/src/lib.rs.txt) | `execute_export`、环境、闭包、primitive、capability 与 `text@1` |

`Value::Int` 使用 `num_bigint::BigInt`；任何 FFI 或 backend 都不能在没有版本化语言变更时把它收窄成 `i64`。

## Web DSL 与事务

| 文件 | 看什么 |
| --- | --- |
| [service/lib.rs](/source/rust/crates/yanshu-service/src/lib.rs.txt) | method/path 匹配、request Map、response 校验、capability trait、事务 KV |
| [service.yan](/source/examples/tasks/service.yan.txt) | 真实 Schema、route 与五个 CRUD handler |
| [scenarios.json](/source/examples/tasks/scenarios.json.txt) | 11 个有状态业务契约 |
| [费用审批 service.yan](/source/examples/expenses/service.yan.txt) | v2 的可读业务规则与有界集合处理 |
| [费用审批 scenarios.json](/source/examples/expenses/scenarios.json.txt) | enum 白名单、金额累计、数字 key 与除零降级 |

一次请求的语义顺序：固定 Program → 匹配 route → 创建 working copy → 调用 export → 验证 response → commit 或 discard。

## 版本与 AI 演化

| 文件 | 看什么 |
| --- | --- |
| [store/lib.rs](/source/rust/crates/yanshu-store/src/lib.rs.txt) | 极小公共入口、内容 hash 与稳定诊断 |
| [store/store.rs](/source/rust/crates/yanshu-store/src/store.rs.txt) | 注册/晋升/回滚、锁、journal 提交与幂等恢复 |
| [store/transaction.rs](/source/rust/crates/yanshu-store/src/transaction.rs.txt) | 只允许 register/activate 的有界 pending journal schema |
| [store/recovery.rs](/source/rust/crates/yanshu-store/src/recovery.rs.txt) | journal 持久化顺序、逐阶段重放与冲突拒绝 |
| [store/events.rs](/source/rust/crates/yanshu-store/src/events.rs.txt) | legacy event 兼容、v2 sequence/hash chain 与生命周期验证 |
| [store/metadata.rs](/source/rust/crates/yanshu-store/src/metadata.rs.txt) | metadata schema、大小、provider/report 与原子 JSON 边界 |
| [store/storage.rs](/source/rust/crates/yanshu-store/src/storage.rs.txt) | 有界非 symlink 读取、同步临时文件、原子替换与平台持久化边界 |
| [store/scenario.rs](/source/rust/crates/yanshu-store/src/scenario.rs.txt) | 注册、晋升、重启读取和回滚生命周期 |
| [provider/lib.rs](/source/rust/crates/yanshu-provider/src/lib.rs.txt) | provider trait、OpenAI/DeepSeek adapter、HTTPS、大小/超时、密钥零化 |
| [provider/agent.rs](/source/rust/crates/yanshu-provider/src/agent.rs.txt) | Codex/Claude Code/OpenCode 非交互适配、一次性候选目录、权限/超时/输出边界 |
| [cli/main.rs](/source/rust/crates/yanshu-cli/src/main.rs.txt) | deploy/evolve 如何组合 Parser、suite 和 VersionStore |
| [ops/operations.rs](/source/rust/crates/yanshu-ops/src/operations.rs.txt) | 离线 backup/verify/restore 的失败关闭流程 |
| [ops/manifest.rs](/source/rust/crates/yanshu-ops/src/manifest.rs.txt) | snapshot schema、逐文件 SHA-256/大小与路径限制 |
| [ops/lease.rs](/source/rust/crates/yanshu-ops/src/lease.rs.txt) | server 生命周期 service lock 与维护互斥 |
| [rollout/policy.rs](/source/rust/crates/yanshu-rollout/src/policy.rs.txt) | 固定候选 hash 与 request ID 确定性采样 |
| [rollout/comparison.rs](/source/rust/crates/yanshu-rollout/src/comparison.rs.txt) | 只在内存比较状态、handler、错误与内容摘要 |
| [rollout/observation.rs](/source/rust/crates/yanshu-rollout/src/observation.rs.txt) | 不含请求/响应内容和内容指纹的 JSONL schema |

`evolve_service_with_provider` 是最短的控制流入口：读 active → 测当前版本 → 请求候选 → 解析候选 → 测完整 suite → 注册 → 可选晋升。

运维快照不包含 observations；它校验版本源码、metadata、active、事件 sequence/hash chain 和可选 KV v1 文档，并拒绝执行快照中夹带的 pending journal。恢复始终拒绝覆盖既有 code/data 目标。VersionStore 的崩溃恢复协议见[设计文档](/source/docs/engineering/version-store-recovery.md.txt)。

## Rust 请求安全链

按一个 HTTP 请求的真实顺序阅读：

1. [server/main.rs](/source/rust/crates/yanshu-server/src/main.rs.txt)：读取参数与可选 token，拒绝非 loopback bind；
2. [http/transport.rs](/source/rust/crates/yanshu-http/src/transport.rs.txt)：HTTP/1 连接、header deadline 与 graceful shutdown；
3. [http/request.rs](/source/rust/crates/yanshu-http/src/request.rs.txt)：target/header/body 上限、敏感 header 过滤、path/query 解码；
4. [http/auth.rs](/source/rust/crates/yanshu-http/src/auth.rs.txt) 与 [loader.rs](/source/rust/crates/yanshu-http/src/loader.rs.txt)：Bearer 摘要校验与 active hash 固定；
5. [http/router.rs](/source/rust/crates/yanshu-http/src/router.rs.txt) 与 [dispatch.rs](/source/rust/crates/yanshu-http/src/dispatch.rs.txt)：并发准入、宿主 request ID 与执行编排；
6. [service/lib.rs](/source/rust/crates/yanshu-service/src/lib.rs.txt)：匹配 route、执行事务 handler；
7. [http/response.rs](/source/rust/crates/yanshu-http/src/response.rs.txt)：响应大小和 guest framing/authentication header 拒绝；
8. 回到 `dispatch.rs` 加入 `X-Request-Id`；[http/observation.rs](/source/rust/crates/yanshu-http/src/observation.rs.txt) 将白名单观测字段同步落盘；
9. [http/shadow.rs](/source/rust/crates/yanshu-http/src/shadow.rs.txt)：有界后台候选读取请求前快照，比较后丢弃副作用。

观测字段白名单是 `schemaVersion/timestampMs/requestId/method/status/durationMs/handler/version/errorCode`。测试使用特殊秘密值确认 path、query、header、body 和内部诊断不会进入 JSONL。

## Library Backend

v0.9 的 `text@1` contract、类型和 fuel 模型在 [library/contract.rs](/source/rust/crates/yanshu-library/src/contract.rs.txt)，安全 Rust 实现在 [library/text.rs](/source/rust/crates/yanshu-library/src/text.rs.txt)，注册、形状校验与错误截断在 [library/registry.rs](/source/rust/crates/yanshu-library/src/registry.rs.txt)。解释器负责在调用前扣除 fuel，并对返回值继续执行 portable 边界校验。

自定义安全 Rust provider trait 已实现；稳定 FFI、动态加载、WASM/sidecar backend 尚未实现。路线见[标准库与 Library Backend](/language/standard-library)和[Rust 宿主与生态路线](/development/rust-roadmap)。

## Workspace 与安全策略

| 文件 | 用途 |
| --- | --- |
| [Cargo.toml](/source/Cargo.toml.txt) | workspace 成员、Rust 1.97、依赖版本、`unsafe_code = forbid` |
| [Cargo.lock](/source/Cargo.lock.txt) | 可复现依赖锁 |
| [deny.toml](/source/deny.toml.txt) | advisory、license、source 与 duplicate 策略 |
| [rust-safety-policy.md](/source/docs/engineering/rust-safety-policy.md.txt) | 第一方 unsafe 与依赖审计边界 |
| [rust-dependency-audit.md](/source/docs/engineering/rust-dependency-audit.md.txt) | 第三方依赖 unsafe 清单与复核记录 |
| [CI workflow](/source/.github/workflows/ci.yml.txt) | Windows/Linux Rust、依赖、conformance、WASM 与 Wiki 门禁 |
| [Fuzz workflow](/source/.github/workflows/fuzz.yml.txt) | 定时/按需的不可信输入 libFuzzer 预算 |
| [Release workflow](/source/.github/workflows/release.yml.txt) | 标签身份、双构建、SBOM、checksum、keyless provenance 与发布权限 |
| [发布构建器](/source/scripts/build-release.mjs.txt) | clean source 绑定、双 target-dir 字节比对、MSVC `/Brepro` 与确定性 ZIP |
| [发布组装器](/source/scripts/assemble-release.mjs.txt) | 两个平台、SBOM、构建记录、manifest 与 SHA-256 闭包 |
| [发布验证器](/source/scripts/verify-release.mjs.txt) | 下载目录的 checksum、大小和 manifest 覆盖校验 |
| [仓库边界检查](/source/scripts/check-repository-boundaries.ps1.txt) | 第一方 safe Rust、Action SHA 与已跟踪凭据模式 |
| [本地总门禁](/source/scripts/check.ps1.txt) | 组合源码边界、fmt、workspace test、Clippy、fuzz、发布工具与文档链接检查 |
| [Reader/Parser fuzz](/source/fuzz/fuzz_targets/reader_parser.rs.txt) | UTF-8 source 到 Reader/Parser 的崩溃入口 |
| [portable value fuzz](/source/fuzz/fuzz_targets/portable_value.rs.txt) | 任意 JSON 到有界 guest value 的转换入口 |
| [artifact fuzz](/source/fuzz/fuzz_targets/artifact_loaders.rs.txt) | bytecode/WASM artifact loader 的任意字节入口 |
| [structured boundary fuzz](/source/fuzz/fuzz_targets/boundary_inputs.rs.txt) | Bundle/package 文档与 HTTP normalization 的不可信输入入口 |

v0.11 的 CI/fuzz 威胁模型、时间/内存预算和本地命令见[持续验证规格](/source/docs/specs/v0.11.md.txt)，历史审计项与回归证据见[审计收口矩阵](/source/docs/specs/v0.11-audit-closure.md.txt)；发布身份与真实性边界见[可验证发布](/development/releases)。
