# 实现架构

语言语义与宿主实现是两层。`.ail` 定义可移植 Program、Value、Schema、route 和 capability；当前 Rust workspace 负责把这些语义实现成受限解释器、业务服务与版本控制面。

## 总体数据流

```text
                 不可信输入
       ┌─────────────────────────────┐
       │ .ail source   LLM candidate │
       └──────┬──────────────┬───────┘
              │              │
              ▼              │
     Reader → Parser → typed AST
              │              │
              ▼              │
       bounded interpreter    │
              │              │
              ▼              │
       complete test suites ◄─┘
              │ passed
              ▼
       immutable version store
              │ explicit promote
              ▼
          active pointer
              │ pinned per request
              ▼
 HTTP → route → handler → validated response → commit transaction
```

所有箭头的规则都由可信宿主掌握；候选和请求始终作为数据进入。

## Crate 分层

| crate | 责任 | 关键边界 |
| --- | --- | --- |
| `ail-diagnostic` | 稳定 code/message/details 与私有 source span | 公共错误不泄漏内部诊断 |
| `ail-syntax` | 有边界 Reader、Parser、AST、Schema/route 元数据 | 未知语法在执行前拒绝 |
| `ail-runtime` | Value、词法环境、闭包、primitive、Schema、fuel/depth | 只执行自己的 AST |
| `ail-conformance` | 读取 canonical fixture 并生成报告 | 固定 portable value / diagnostic |
| `ail-service` | route dispatch、capability、事务 KV、响应验证 | 合法响应后才提交 |
| `ail-store` | 内容寻址版本、metadata、active、事件、锁 | 不可变源码与原子指针 |
| `ail-ops` | 离线备份、manifest 校验、拒绝覆盖恢复、service lease | 运行服务与维护操作互斥 |
| `ail-rollout` | 影子采样、候选加载、结果比较与脱敏观测 | 候选副作用与主响应隔离 |
| `ail-provider` | 离线/在线候选、HTTPS、响应验证、密钥保护 | provider 只能返回候选 |
| `ail-http` | Axum/Tokio HTTP、限制、认证、观测 | transport 与 guest 分离 |
| `ail-server` | 独立本地 server 进程 | loopback-only 与优雅关闭 |
| `ail-cli` | check、conformance、部署与演化命令 | JSON 输出与退出码 |

源码入口见[源码地图](/reference/source-map)。

## 1. 语言前端

[Reader](/source/rust/crates/ail-syntax/src/reader.rs.txt) 读取 UTF-8 源码并限制节点与嵌套；[Parser](/source/rust/crates/ail-syntax/src/parser.rs.txt) 检查顶层声明、名称唯一性、capability、library、Schema、route、export 和表达式；[AST](/source/rust/crates/ail-syntax/src/ast.rs.txt) 保存显式结构与 source span。

```rust
pub enum ExpressionKind {
    Literal(/* ... */),
    Variable(/* ... */),
    If(/* ... */),
    Let(/* ... */),
    Function(/* ... */),
    Do(/* ... */),
    Call(/* ... */),
}
```

宿主不执行候选原生代码，只解释已验证 AST。第一方 crate 统一 `#![forbid(unsafe_code)]`。

## 2. 执行内核

[ail-runtime](/source/rust/crates/ail-runtime/src/lib.rs.txt) 以树遍历方式执行表达式。环境和闭包通过受检查 arena index 表示，不使用自引用裸指针。

执行上下文包含：

- `Budget { fuel, maximum_depth }`；
- 词法环境与闭包 arena；
- 按声明安装的 primitive；
- 宿主注入的 capability；
- 精确版本的 Library Backend。

`Value::Int` 使用 `BigInt`，不能静默改成 `i64`。Closure 和 Primitive 不能越过 portable JSON 边界。

## 3. Service 与事务

[ail-service](/source/rust/crates/ail-service/src/lib.rs.txt) 把 Web 请求转换为不可变 guest Map，按 method + path 匹配静态 route，再调用导出的 handler。

```text
store snapshot → working copy → handler
                                │
                 ┌──────────────┴──────────────┐
                 │合法 response               │诊断 / 非法 response
                 ▼                             ▼
               commit                       discard
```

service 层验证 response 必须只有 `status`、`headers`、`body`，并限制 status、header 与 JSON 输出。guest 看不到真实数据库连接或文件句柄，只能调用窄 `kv` capability。

## 4. 版本与演化控制面

[ail-store](/source/rust/crates/ail-store/src/lib.rs.txt) 使用源码 SHA-256 作为版本 ID，保存不可变源码、metadata、active pointer 和事件。写入使用原子文件替换、跨进程锁和有界锁等待。[ail-ops](/source/rust/crates/ail-ops/src/lib.rs.txt) 在这一层之上提供离线快照、逐文件 hash 与语义校验，以及拒绝覆盖的恢复；server 持有 service lease，避免运行与维护并发。

[ail-provider](/source/rust/crates/ail-provider/src/lib.rs.txt) 负责请求 LLM 候选，但 provider 返回后仍要重新经过 Parser 和完整 suite。[ail-cli](/source/rust/crates/ail-cli/src/main.rs.txt) 的 `evolve-service` 默认不晋升；`--promote` 也不能绕过失败报告。

[ail-rollout](/source/rust/crates/ail-rollout/src/lib.rs.txt) 让尚未晋升的固定候选读取请求前 KV 快照，并在后台内存 store 中执行。比较器只把状态、handler、错误码与差异类别交给观测层；候选写入、guest log、响应内容和内容摘要全部丢弃。

## 5. HTTP 请求路径

1. `ail-server` 只接受 loopback bind，可选检查 Bearer。
2. `ail-http` 生成宿主持有的 request ID 并过滤敏感 header。
3. program loader 读取一次 active hash、验证源码完整性并解析 Program。
4. service 匹配 route，为请求创建 KV working copy。
5. runtime 在 fuel/depth 内执行 handler。
6. service 验证 response；成功才提交事务。
7. HTTP adapter 编码响应，并写入使用同一 request ID 和固定版本 hash 的脱敏观测。
8. 若请求被采样，后台候选使用第 4 步之前抓取的 KV 快照执行并写入独立影子观测。

请求开始后即使 active pointer 改变，当前请求仍使用已加载 Program；新请求才会看到新版本。

## 6. Library Backend 边界

```text
.ail `(libraries (text 1))`
          ↓
contract：函数 / 类型 / fuel / portable result
          ↓
host-selected trusted backend
```

当前 `text@1` 参考 backend 位于 `ail-runtime`。guest 不能指定 crates.io package 或动态库。crate 发布、外部 provider trait、WASM/进程隔离和 FFI 都属于后续生态路线，见[标准库](/language/standard-library)与[Rust 宿主路线](/development/rust-roadmap)。

## 当前边界

当前实现适合语言与本地业务原型验证，还不是公网生产平台：

- Bearer 是可选单 token，不是细粒度授权；
- file KV 不是正式数据库；
- blocking guest 工作尚无可安全取消的进程级墙钟隔离；
- JSONL 尚无生产轮转、保留、聚合和告警；
- 没有静态网页交付、TLS、正式数据库 PITR、异地备份或自动 canary。

这些缺口必须在宿主层解决，不能把责任推给 `.ail` 程序或模型。
