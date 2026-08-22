# Rust 宿主与生态路线

这一页只讨论 `.yan` 语言的当前 Rust 实现和后续生态，不定义语言本身。语法、Value、Schema、route、diagnostic 与版本格式应先由语言规格固定，宿主只能实现这些契约。

::: warning 当前定位
Rust v0.10 已形成通用语言的安全内核；v0.11 正在把跨平台 CI、依赖审计、conformance、WASM smoke、不可信输入 fuzz，以及双构建/SBOM/checksum/keyless provenance 固化为持续门禁。项目仍未生产就绪，workspace 的 crate 仍是 `publish = false`，没有稳定 FFI，也没有 crates.io 发布物。
:::

## 当前实现快照

| 层 | 已实现 | 尚未完成 |
| --- | --- | --- |
| 语言前端 | 有边界 UTF-8 Reader、v1-v4 AST / Parser、imports、typed 封闭数据、模式匹配、signature、版本门控、source span、JSON inspect、稳定 expression-v1 节点路径、最小 LSP full sync | 泛型声明、增量语法树、跨任意结构编辑的节点 identity reconciliation |
| 静态分析 | 内部类型推断、export 输入/输出门禁、传递 effect/capability 闭包、已知高阶 callback、失败关闭未知 callback | effect polymorphism、增量分析、LSP |
| 模块/包制品 | Bundle manifest、module/root SHA-256、依赖图、命名空间链接、package store、精确 lock、锁定 capability closure | 签名、远端 registry、离线镜像导入 |
| 审查 | Rust 风格只读投影、definition ID、模块/span/type/effect nodes、VS Code 无脚本旁侧面板 | 结构化编辑明确推迟到 v0.10 之后 |
| 运行时 | BigInt、闭包、递归、短路条件、有界集合、Result、enum/union Schema、校验成本、可替换 Rust `text@1` Backend、验证式字节码、跨解释器/VM 一致的语义 fuel、WASM handle ABI | 原生 WASM lowering、独立内存配额、可安全取消的进程级 deadline |
| 业务服务 | route dispatch、response 校验、事务内存/文件 KV、固定时钟与日志、11 个场景 | 正式数据库 adapter、migration、连接池 |
| 版本库 | SHA-256、不可变校验、有界恢复 journal、幂等重放、v2 event hash chain、原子写、跨进程锁、回滚 | 有密钥签名、透明日志、远端 artifact store、生产审批流 |
| 运维快照 | 离线 service lock、逐文件 SHA-256 manifest、版本/KV 语义校验、拒绝覆盖恢复 | 加密、签名、异地复制、定期恢复演练 |
| Provider | OpenAI Responses、DeepSeek Chat、HTTPS-only、拒绝 redirect、大小/超时、密钥零化 | 真实凭据 smoke gate、速率/费用治理 |
| HTTP / rollout | Axum/Tokio、显式 header/body deadline、loopback-only、可选 Bearer、宿主 request ID、每请求固定 hash、脱敏 JSONL、隔离影子采样 | TLS、细粒度授权、进程沙箱、指标/trace/告警、canary、静态网页 |
| 工具 | check/inspect/review、只读 formatter v1/CI check、有界全局/局部符号索引、LSP 诊断/精确 token hover/作用域感知 completion/全文 semantic tokens/同文档 definition/references/防捕获 rename/formatting edit/版本化 review、VS Code 无脚本审查面板、平台专用 VSIX、Windows/Linux Extension Host 验收、Bundle、package pack/lock/verify/review/run/compile、bytecode/WASM compile/inspect/run、conformance、test/deploy/evolve service、version lifecycle、跨平台 CI、边界 fuzz、双构建与 keyless provenance | Tree-sitter、semantic token range/delta、LSP 增量、跨文件导航、更多编辑器、MCP、结构化 AST diff、操作型 rollback CLI |

## Crate 边界

```text
yanshu-diagnostic
      ▲
yanshu-library ─┬─► yanshu-syntax ─► yanshu-analysis ─► yanshu-compiler ─► yanshu-runtime ─► yanshu-service ─► yanshu-http ─► yanshu-server
             │                               └─► yanshu-bundle ─┬─► yanshu-package ─► yanshu-cli
             └───────────────────────────────────────────────┘
                    │              │
                    │              └──────► yanshu-store
                    │                         ▲
                    └────► yanshu-conformance    │
                                              │
yanshu-provider ─────────────────────────────► yanshu-cli
yanshu-store + yanshu-service ─► yanshu-rollout ───► yanshu-http
yanshu-store + yanshu-service ─► yanshu-ops
```

实际依赖以 [Cargo.toml](/source/Cargo.toml.txt) 为准；图表达责任方向，不承诺稳定公共 API。

所有第一方 crate 都继承 `unsafe_code = "forbid"` 并在 crate root 再次 `#![forbid(unsafe_code)]`。这不代表第三方依赖没有内部 unsafe；供应链边界见[依赖审计](/source/docs/engineering/rust-dependency-audit.md.txt)。

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

## 通用语言主路线

路线顺序是正式约束：后一阶段不能以绕过前一阶段安全设计的方式抢跑。

1. **v0.6**：完成条件、集合、Schema、Result 和真实费用场景；
2. **v0.7**：模块、用户定义数据类型、模式匹配、密封 Bundle；
3. **v0.8**：类型与效果系统，静态计算 capability 闭包；
4. **v0.9**：内容寻址包管理、锁文件、Rust Library Backend；
5. **v0.10**：有 fuel 计量的字节码 / WASM 编译器；
6. **v0.11**：跨平台持续验证、fuzz、可复现发布与供应链证据；
7. **v0.12**：Agent 工具协议、formatter、Tree-sitter、LSP 与 MCP；formatter v1、稳定节点路径、有界全局/局部符号索引、精确 token hover、作用域感知 completion、全文 semantic tokens、同文档 definition/references、防捕获 rename、版本化只读审查面板、VS Code 平台包与 Windows/Linux Extension Host 验收已经落地，其余仍在开发；
8. **v0.13+**：标准库扩展，以及受 capability、effect、fuel、取消和确定性约束的结构化并发。

模块必须与 Bundle 根 hash、模块 hash、依赖闭包和 capability 清单一起交付；不能先引入按路径动态加载。包管理不能运行安装脚本。类型系统不能把 fuel、效果或 capability 从 AST 中藏起来。编译器输出仍须通过独立验证器，不能因为“已经编译”就跳过语言门禁。

## crates.io 发布路线

当前 workspace 使用统一版本 `0.10.0`，但 `publish = false`。发布前至少需要：

1. 明确哪些 crate 是公共 API，哪些只服务于 binary；
2. 把 `Program`、`Value`、`Diagnostic`、Library Contract 的兼容性写入 semver 策略；
3. 设置 repository、documentation、readme、keywords、license 与 MSRV 元数据；
4. 设计 feature flags，避免默认拉入 HTTP/provider 等高层依赖；
5. 为公开 API 增加文档测试和跨版本 fixture；
6. 使用 `cargo package` 检查发布内容，确保不含凭据、runtime store 和未授权 source；
7. 建立 release signing、RustSec、license 和 dependency provenance gate。

CLI 的发布供应链 gate 已建立，但 crates.io 仍保持关闭。它提供确定性归档、CycloneDX SBOM、SHA-256 闭包和 GitHub OIDC provenance；具体承诺与非承诺见[可验证发布](/development/releases)。这不等于 crate 公共 API 已达到 semver 稳定要求。

建议首先评估发布：

- `yanshu-diagnostic`：最小稳定诊断模型；
- `yanshu-syntax`：Parser 与 AST，但要先决定 source span / inspect JSON 的兼容承诺；
- `yanshu-runtime`：依赖语义面较大，适合在 conformance 更稳定后发布；
- 高层 server/provider crate 暂缓，避免过早冻结部署接口。

## Library Backend 路线

v0.9 已把 `text@1` 从解释器硬编码迁到独立 `yanshu-library` crate，并提供安全 provider contract：

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

自定义安全 Rust backend 可以显式注册；guest 仍不能指定 provider。外部 backend 优先使用可终止的 sidecar 或 WASM，进程内动态加载放在安全模型稳定之后。

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

## Rust 风格只读审查视图（v0.8 已实现）

面向不熟悉 S 表达式的审查者，分析器会从已验证 AST 单向生成 Rust 风格只读视图：

```rust
// Generated review view — read only.
route!(POST, "/tasks", create_task);

schema! TaskCreate {
    required id: String[1..=64],
    optional completed: Bool = false,
}
```

当前契约：

1. 视图只读，不能反向作为执行源码；
2. 每个显示节点能追踪到 `.yan` source span 和稳定 AST ID；
3. machine-readable node 同屏携带 definition、模块、span、type 和 capability；
4. `.yan` AST、suite 和宿主策略仍是唯一执行真相；
5. 反向转换与结构化编辑明确推迟到 v0.10 之后。

VS Code 通过版本化 `yanshu/reviewDocument` 把这份投影显示在无脚本旁侧面板中。面板没有文本编辑模型；它只消费当前打开快照，并保留 `.yan` 作为唯一可修改输入。

## 生产宿主路线

建议按以下风险顺序推进，不以“server 能启动”作为完成标准：

1. 把同步 guest 工作移入可终止的独立进程，建立内存/CPU/墙钟配额；
2. 引入正式数据库 adapter、migration、备份与恢复演练；
3. 在可信 TLS 入口实现身份、角色、资源级授权和限流；
4. 把 JSONL 接入轮转、保留、访问控制、聚合、指标和告警；
5. 在现有 shadow 之上增加累计阈值、签名 artifact、审批策略、canary 与自动停止条件；
6. 再考虑静态网页交付与默认生产流量。

实现细节入口见[源码地图](/reference/source-map)，语言层边界见[标准库](/language/standard-library)和[安全模型](/evolution/security)。
