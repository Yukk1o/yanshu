# Rust 迁移路线

迁移目标不是把 Lisp 改写成 Rust 业务代码，而是用 Rust 重写**宿主内核**，同时保持 `.ail` 源码、JSON tests、诊断 code、版本文件和行为一致。

## 必须保持不变的契约

- `.ail` 文档语法和求值顺序；
- 只有 `#f` 为假的 truthiness；
- 顺序 `let`、词法闭包和 primitive arity；
- Schema normalization、issue 顺序和稳定 code；
- JSON 输入/输出形状；
- route 匹配、响应校验和错误信封；
- fuel / depth 耗尽的诊断语义；
- service scenarios 与纯函数 test reports；
- SHA-256 source hash、metadata、active pointer 和 event 格式；
- 请求级 active version pinning。

Rust 第一方 crate 还有一条不可放宽的实现约束：全部使用
`#![forbid(unsafe_code)]` 并继承 workspace 的 `unsafe_code = "forbid"`。Library
Backend 不通过裸指针或手写原生 ABI 接入；优先使用安全 crate API、隔离进程协议或
WASM Component。第三方依赖中的 unsafe 无法被本项目 lint 自动禁止，因此必须经过
锁文件、来源、漏洞、许可证和 unsafe inventory 审计。完整政策见
[rust-safety-policy.md](/source/docs/rust-safety-policy.md.txt)。

这组契约比 Racket 函数名更重要。迁移成功的判据是同一输入得到同一 portable output / diagnostic。

## 建议的 Rust workspace

```text
crates/
├─ ail-syntax/       reader、source span、AST、parser
├─ ail-runtime/      Value、Env、Evaluator、Budget、Diagnostic
├─ ail-schema/       Schema validation、normalization、issues
├─ ail-service/      Request/Response、route、capability contracts
├─ ail-store/        version store、KV traits 与 adapters
├─ ail-evolution/    provider、candidate gate、promotion policy
├─ ail-http/         基于成熟 HTTP 库的 transport adapter
├─ ail-server/       活动版本 HTTP 进程入口与优雅关闭
├─ ail-provider/     受控 LLM 请求/响应、HTTPS transport 与凭据边界
└─ ail-cli/          与当前 JSON CLI 兼容的 binary
```

不要一开始就做一个巨型 crate；分层能让核心解释器不依赖 HTTP、数据库或 LLM SDK。

## 核心类型草图

```rust
pub enum Value {
    Nil,
    Bool(bool),
    Int(num_bigint::BigInt),
    String(Arc<str>),
    Symbol(Symbol),
    List(Vec<Value>),
    Map(BTreeMap<MapKey, Value>),
    Ok(Box<Value>),
    Err(Box<Value>),
    Closure(ClosureId),
    Primitive(PrimitiveId),
    Schema(SchemaId),
}

pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: Cow<'static, str>,
    pub details: serde_json::Value,
}

pub struct Budget {
    pub fuel: u64,
    pub max_depth: u32,
}
```

当前 Racket `exact-integer` 是任意精度整数，所以 Rust Value 必须使用
`num_bigint::BigInt`（或语义等价的任意精度实现）。不能为了实现方便静默收窄成
`i64`：这会让原本合法的大整数程序溢出或失败。只有未来通过明确的、版本化的
客体语言变更，并给出迁移和一致性测试后，才可以改变整数范围。

Map 是否用 `BTreeMap` 要以现有可观察顺序为准；不要因为 `HashMap` 更常见而无意改变诊断和序列化稳定性。

## 如何直接复用 Rust 生态

可以直接复用，而且不需要让客体语言重新发明 HTTP、JSON、数据库或密码学库。推荐的边界是：

```text
.ail 调用
   │  稳定的语言级模块 / capability 名称
   ▼
宿主 capability dispatcher
   │  Value、Diagnostic、Budget 与权限检查
   ▼
窄 Rust trait / adapter
   │
   └─► 成熟 crate（Serde、HTTP client、数据库驱动……）
```

例如，Rust 宿主可以用成熟 crate 完成 HTTP transport，但客体只看到项目定义的
`http.request` 数据结构。adapter 负责在 `Value` 与 Rust 类型之间转换，并统一执行域名
白名单、超时、响应大小、调用次数和错误码限制。以后替换底层 crate 时，`.ail` 程序不需要改变。

不应尝试把任意 crate API 自动暴露给客体。Rust API 中的泛型、生命周期、trait、异步
类型和错误类型不是稳定的语言 ABI，而且这样做会绕过 capability 权限模型。每类生态能力
只需要一层很薄、经过审查的 adapter：

| 需求 | Rust 宿主复用的生态 | 暴露给 `.ail` 的边界 |
| --- | --- | --- |
| JSON / 配置 | Serde 生态 | `Value` 转换与稳定诊断 |
| HTTP client/server | Tokio 上的成熟 HTTP 库 | URL 白名单、deadline、大小与次数预算 |
| 数据库 | 成熟驱动或查询层 | 请求级事务、参数化操作，不暴露连接对象 |
| 正则、URL、编码 | 对应的纯函数 crate | 有输入上限的纯函数模块 |
| 密码学 | 经过审计的 crate | 用途明确的高层操作，不暴露 key material |

第一阶段采用**静态链接的宿主模块**：依赖由 `Cargo.lock` 固定，随 Rust host 一起测试和
发布。这最简单，也最符合当前可信内核模型。客体程序可以动态更新，但不能在运行期自行
下载 crate 或修改 `Cargo.toml`；AI 只能提出新增 capability 的候选，新增宿主依赖仍需人工
审查、依赖审计和重新构建宿主。

如果以后确实需要第三方热插拔插件，优先定义版本化的
[WebAssembly Component Model / WASI](https://component-model.bytecodealliance.org/) 接口，
让插件在独立沙箱中运行。不要把 Rust `dylib` 当公共插件 ABI：Rust 本身不保证稳定 ABI，
升级编译器或依赖后可能失配。还有一种离线方案是由工具生成 Rust/Cargo 工程并正常编译，
它适合部署优化，不适合在服务请求期间即时编译和加载。

因此，“复用 Rust 生态”和“运行期演化”并不冲突：热更新的是受限 `.ail` 业务层，稳定、
高权限、依赖丰富的 Rust 能力层按常规供应链流程升级。两层通过小而稳定的契约连接。

## 阶段 0：冻结一致性语料

::: tip 已落地
v1 manifest、portable value codec 和 Racket runner 已位于
[`conformance/v1/manifest.json`](/source/conformance/v1/manifest.json.txt) 与
[`src/conformance-suite.rkt`](/source/src/conformance-suite.rkt.txt)。可运行
`conformance conformance/v1/manifest.json` 验证当前 oracle。
:::

在写 Rust 前先把 Racket 行为变成 golden/conformance fixtures：

- 每种 AST form 的成功结果；
- Reader / Parser 所有稳定错误 code；
- arity、类型、fuel、depth 错误；
- JSON round-trip；
- Schema issue 顺序、上限和 JSON Pointer；
- route overlap、405 Allow、事务 rollback；
- version store 文件与事件。

当前 [tests/all.rkt](/source/tests/all.rkt.txt) 是起点，但应提取宿主无关 JSON fixture，让 Racket 和 Rust runner 同时消费。

## 阶段 1：Reader + AST + Parser

::: tip 已落地第一版
Rust workspace 已包含 `ail-diagnostic`、`ail-syntax` 与只读 `ail-cli`。三个
conformance program 的完整 inspect JSON，以及两个无效输入的 diagnostic，均已通过
Racket/Rust 精确差分。运行 `scripts/check-rust.ps1` 可同时检查格式、测试、Clippy、
unsafe 门禁与前端差分。
:::

先实现纯前端：`.ail source → Program AST / Diagnostic`，输出与 `cli inspect` 可比较的 JSON。

注意：不能直接选择功能完整的通用 Scheme reader，然后暴露项目没定义的 datum。字符串、整数、布尔、符号、proper list 和源大小限制需要明确。

## 阶段 2：纯解释器

::: tip 已通过 v1 语料
`ail-runtime` 已实现闭包/递归、顺序 let、任意精度整数、纯 primitives、fuel/depth、
Schema validator 与 `text@1`。`ail-conformance` 读取与 Racket 相同的 17-case manifest，
目前整份报告 canonical JSON 零差异。checkpoint 见
[rust-host-v0.5.md](/source/docs/rust-host-v0.5.md.txt)。
:::

只实现无 capability 的 pure primitives，跑 discount suite。Evaluator API 应显式接收 `&mut Budget`，每个节点和调用点按现有规则扣 fuel。

避免在 Rust 中使用 panic 表达客体错误；所有预期失败都返回 `Result<Value, Diagnostic>`。

## 阶段 3：Schema + service + capabilities

::: tip 已落地
Schema AST、normalization、issue 顺序和 fuel 已在 Rust 中落地。`ail-service` 也已用显式
capability trait 接入 route、request/response、内存事务 KV、固定 clock/log，并通过现有
11 个任务场景；Racket/Rust service report canonical JSON 零差异。文件 KV 已兼容旧版
`version: 1` JSON，采用同目录临时文件、`sync_all` 与标准库 `rename` 替换，并验证重启恢复、
损坏文件拒绝、失败不提交内存状态及临时文件清理。
:::

加入 Schema AST/validator、route、request/response 和 capability traits。先使用内存 KV adapter 重放 11 个任务场景，再实现与旧文件格式兼容的 adapter。

Capability dispatcher 应采用白名单注册，不能让 guest 通过字符串反射调用任意 Rust 函数。
这一阶段同时建立上述 crate adapter 规范；先接入一两个纯函数模块验证类型转换、预算和稳定错误，不要一开始就开放网络与文件系统。

## 阶段 4：HTTP、版本库和 provider

::: tip 版本库、HTTP 与 provider 已落地
`ail-store` 已兼容 Racket 的 SHA-256 内容地址、元数据、活动指针与事件序列，并增加源码
完整性校验、hash 路径约束、标准库跨进程文件锁和有界锁超时。Racket/Rust 会执行同一套
注册、测试门禁、晋升、重启读取与回滚生命周期，canonical JSON 当前零差异。

`ail-http` 使用 Axum/Tokio，把有界 HTTP 请求转换为同一个 `ServiceRequest`；
`ail-server` 在每次请求开始时从兼容版本库读取并固定活动程序。测试覆盖进程内路由和
真实 loopback TCP 连接。Rust CLI 的 `deploy-service` 会先跑完 11 个业务场景，只有
通过才晋升，因此可从空版本目录启动，不依赖 Racket 预部署。

`ail-provider` 已迁移 OpenAI Responses 与 DeepSeek Chat 的严格请求/响应边界，并用
Reqwest/Rustls 提供仅 HTTPS、无重定向、有墙钟和大小限制的控制面 transport。Rust CLI
的 `evolve-service` 默认只注册测试后的候选；只有显式 `--promote` 且 11 个场景全通过
才修改 active。当前测试使用模拟 transport；真实联网仍要求操作者配置环境凭据。
:::

HTTP 使用成熟 Rust 生态库，而不是逐行移植手写 TCP parser；把请求转换成稳定的 `ServiceRequest` 后再进入宿主无关 service 层。

provider 层已经最后迁移，因为它在信任边界之外，没有阻塞语言一致性。后续重点是把
真实 provider replay、shadow traffic 和晋升审批纳入发布门禁。

## 阶段 5：生成只读审查视图（建议）

::: warning 路线建议，尚未实现
当前项目只有 JSON AST inspect，没有 Rust 风格审查视图。此阶段是为了帮助不熟悉 Lisp 的维护者，不是迁移必需条件，也不是第二种执行语法。
:::

从已验证 AST 生成 Rust 风格伪代码，并同时展示结构化差异：

- capability added/removed；
- route added/removed/changed；
- Schema required/optional/bounds/default diff；
- error code/status diff；
- KV write/delete call sites；
- source span 与 AST node 的双向定位。

生成视图必须只读、不可反向编译；执行真相仍是 `.ail` AST。详见[如何审查 AI 生成的改动](/evolution/review-ai-change#路线建议-rust-风格只读审查视图)。

## 阶段 6：双运行与切换

在同一 fixtures 上并行运行 Racket/Rust：

```text
fixture ─┬─► Racket host ─► result/diagnostic A
         └─► Rust host   ─► result/diagnostic B
                                  │
                            canonical JSON compare
```

做到零差异后，先把 Rust 用作 shadow evaluator，再承接本地服务，最后替换默认 CLI。Racket 版本保留一段时间作为语义 oracle。

## 阶段 7：有限自举

::: warning 长期路线，当前语言尚不能自举
目前的 `.ail` 适合表达受限业务函数，不具备实现完整编译器所需的模块、字节序列、丰富
数据结构、AST 构造/匹配和制品输出能力。不要把“AI 能生成候选”误称为“语言已经自举”。
:::

推荐把自举目标分成两部分：

- 用 `.ail` 实现 formatter、lint、静态分析、AST rewrite，逐步验证语言处理语言自身的能力；
- 最终用 `.ail` 实现编译器前端或字节码编译器，但把权限、预算、签名、制品验证和 OS
  适配器继续留在小型 Rust 可信内核中。

典型 bootstrap 链如下：

```text
Rust Stage-0 解释器
    │ 运行 ailc.ail
    ▼
Stage-1 编译器制品
    │ 再编译同一份 ailc.ail
    ▼
Stage-2 编译器制品 ── canonical / byte-for-byte compare ── Stage-1
```

Stage-1 与 Stage-2 固定点、Racket/Rust 差分语料、可复现构建和签名共同构成自举验收。
即使达到这一步，启动系统仍需一个可审计的 seed；保留小型 Rust Stage-0 是正常的，类似
其他自举编译器仍需要已有二进制或更早阶段工具链。

这里追求的是**自举编译器**，不是让当前活动程序原地改写解释器。AI 生成的新编译器仍是
候选制品，必须通过完整一致性测试、两阶段固定点检查和显式晋升，不能修改正在执行它的
可信根。这使“代码处理代码”与供应链安全可以同时成立。

## 为什么不优先迁 Go

Go 也能快速实现清晰服务，团队熟悉 Go 时完全可行；但这个项目的长期内核包含递归 AST、枚举 Value、精细错误传播、capability 生命周期与不可信执行边界，Rust 的 enum、pattern matching、`Result`、所有权和无数据竞争并发更贴合。

Go 更适合快速重写 HTTP/control plane；Rust 更适合把解释器和能力内核做成可长期收紧的可信基座。当前路线选择 Rust，但一致性 fixtures 会让宿主选择保持可逆。
