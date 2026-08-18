# Rust 宿主与生态路线

这一页只讨论 `.ail` 语言的当前 Rust 实现和后续生态，不定义语言本身。语法、Value、Schema、route、diagnostic 与版本格式应先由语言规格固定，宿主只能实现这些契约。

::: warning 当前定位
Rust v0.5 已能独立运行语言、业务场景、版本库、provider 和本地 JSON HTTP server，但尚未生产就绪。workspace 的 crate 仍是 `publish = false`，没有稳定 FFI，也没有 crates.io 发布物。
:::

## 当前实现快照

| 层 | 已实现 | 尚未完成 |
| --- | --- | --- |
| 语言前端 | 有边界 UTF-8 Reader、完整 AST / Parser、source span、JSON inspect | 增量解析、稳定 AST ID、格式化器 |
| 运行时 | BigInt、词法闭包、递归、顺序 let、fuel/depth、primitive、Schema、`text@1` | 独立内存配额、可安全取消的进程级 deadline |
| 业务服务 | route dispatch、response 校验、事务内存/文件 KV、固定时钟与日志、11 个场景 | 正式数据库 adapter、migration、连接池 |
| 版本库 | SHA-256、不可变校验、metadata、active、events、原子写、跨进程锁、回滚 | 签名、远端 artifact store、生产审批流 |
| 运维快照 | 离线 service lock、逐文件 SHA-256 manifest、版本/KV 语义校验、拒绝覆盖恢复 | 加密、签名、异地复制、定期恢复演练 |
| Provider | OpenAI Responses、DeepSeek Chat、HTTPS-only、拒绝 redirect、大小/超时、密钥零化 | 真实凭据 smoke gate、速率/费用治理 |
| HTTP / rollout | Axum/Tokio、loopback-only、可选 Bearer、宿主 request ID、每请求固定 hash、脱敏 JSONL、隔离影子采样 | TLS、细粒度授权、进程沙箱、指标/trace/告警、canary、静态网页 |
| 工具 | check/inspect、conformance、test/deploy/evolve service、version lifecycle | 包管理、LSP、formatter、结构化 AST diff、操作型 rollback CLI |

## Crate 边界

```text
ail-diagnostic
      ▲
ail-syntax ─► ail-runtime ─► ail-service ─► ail-http ─► ail-server
                    │              │
                    │              └──────► ail-store
                    │                         ▲
                    └────► ail-conformance    │
                                              │
ail-provider ─────────────────────────────► ail-cli
ail-store + ail-service ─► ail-rollout ───► ail-http
ail-store + ail-service ─► ail-ops
```

实际依赖以 [Cargo.toml](/source/Cargo.toml.txt) 为准；图表达责任方向，不承诺稳定公共 API。

所有第一方 crate 都继承 `unsafe_code = "forbid"` 并在 crate root 再次 `#![forbid(unsafe_code)]`。这不代表第三方依赖没有内部 unsafe；供应链边界见[依赖审计](/source/docs/rust-dependency-audit.md.txt)。

## 语言语义的硬约束

Rust 实现不能为了方便静默改变：

- `Int` 的任意精度语义：必须保留 `num_bigint::BigInt`，不能收窄到 `i64`；
- 只有 `#f` 为假的 truthiness；
- Nil、List、Map、Symbol、Result 的 portable codec；
- `let` 从左到右、后绑定可引用前绑定；
- fuel、调用深度与 Schema issue 顺序；
- 稳定 diagnostic code/message/details；
- route、response、事务提交和 request-level version pinning；
- 内容哈希版本、parent 和 active pointer 语义。

任何破坏性调整都应成为显式语言版本，而不是内部重构的副作用。

## crates.io 发布路线

当前 workspace 使用统一版本 `0.5.0`，但 `publish = false`。发布前至少需要：

1. 明确哪些 crate 是公共 API，哪些只服务于 binary；
2. 把 `Program`、`Value`、`Diagnostic`、Library Contract 的兼容性写入 semver 策略；
3. 设置 repository、documentation、readme、keywords、license 与 MSRV 元数据；
4. 设计 feature flags，避免默认拉入 HTTP/provider 等高层依赖；
5. 为公开 API 增加文档测试和跨版本 fixture；
6. 使用 `cargo package` 检查发布内容，确保不含凭据、runtime store 和未授权 source；
7. 建立 release signing、RustSec、license 和 dependency provenance gate。

建议首先评估发布：

- `ail-diagnostic`：最小稳定诊断模型；
- `ail-syntax`：Parser 与 AST，但要先决定 source span / inspect JSON 的兼容承诺；
- `ail-runtime`：依赖语义面较大，适合在 conformance 更稳定后发布；
- 高层 server/provider crate 暂缓，避免过早冻结部署接口。

## Library Backend 路线

当前只有内置 `text@1`。下一步不应让 guest 直接写 Cargo dependency，而应抽出稳定 provider contract：

```rust
trait LibraryBackend {
    fn contract(&self) -> LibraryContract;
    fn invoke(
        &self,
        operation: &str,
        args: &[PortableValue],
        budget: &mut Budget,
    ) -> Result<PortableValue, Diagnostic>;
}
```

接口设计必须保留：

- 精确 library name + version；
- 允许操作、参数/结果类型和 fuel 费用；
- BigInt 的无损表示；
- 结果节点/深度/字符串上限；
- 确定性与副作用声明；
- backend error 到稳定 diagnostic 的映射；
- conformance fixture 与恶意 backend 测试。

外部 backend 优先使用可终止的 sidecar 或 WASM，进程内动态加载放在安全模型稳定之后。

## FFI 路线

::: warning 尚未实现
项目目前没有 `extern "C"` 导出、稳定 ABI、头文件或语言绑定。Rust enum 的内存布局不是公共协议。
:::

如果未来为 Go、Python 或其它宿主提供 FFI，建议使用：

- 不透明句柄，不暴露 `Program` / `Value` 内存布局；
- UTF-8 pointer + explicit length，而不是依赖 NUL 终止；
- 明确的 allocator 所有权与释放函数；
- JSON / CBOR 等版本化 portable envelope，BigInt 使用无损十进制；
- panic 全部截断在 ABI 内，返回结构化 diagnostic；
- 每次调用显式传入 budget、capability set 和 library registry；
- 多线程、取消、句柄生命周期与 ABI version 的 conformance tests。

FFI 不能成为绕过 Parser、fuel、capability 或版本门禁的后门。

## 阶段 5：生成只读审查视图（建议）

::: warning 尚未实现
当前 `inspect` 只输出 JSON AST；不会生成 Rust 风格代码，也没有专用审查 UI。
:::

面向不熟悉 S 表达式的审查者，可以从已验证 AST 单向生成 Rust 风格只读视图：

```rust
// Generated review view — read only.
route!(POST, "/tasks", create_task);

schema! TaskCreate {
    required id: String[1..=64],
    optional completed: Bool = false,
}
```

设计要求：

1. 视图只读，不能反向作为执行源码；
2. 每个显示节点能追踪到 `.ail` source span 和稳定 AST ID；
3. 同屏突出 route、Schema、capability、错误码和写入操作 diff；
4. 无法等价表达的节点必须显式标记，不能静默简化；
5. `.ail` AST、suite 和宿主策略仍是唯一执行真相。

## 生产宿主路线

建议按以下风险顺序推进，不以“server 能启动”作为完成标准：

1. 把同步 guest 工作移入可终止的独立进程，建立内存/CPU/墙钟配额；
2. 引入正式数据库 adapter、migration、备份与恢复演练；
3. 在可信 TLS 入口实现身份、角色、资源级授权和限流；
4. 把 JSONL 接入轮转、保留、访问控制、聚合、指标和告警；
5. 在现有 shadow 之上增加累计阈值、签名 artifact、审批策略、canary 与自动停止条件；
6. 再考虑静态网页交付与默认生产流量。

实现细节入口见[源码地图](/reference/source-map)，语言层边界见[标准库](/language/standard-library)和[安全模型](/evolution/security)。
