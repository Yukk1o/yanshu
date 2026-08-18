# AI-Evolve

> 更习惯 Go / Rust、暂时看不懂 Lisp？从 [中文 Wiki](wiki/README.md) 开始，
> 其中包含 5 分钟上手、逐段语法翻译、架构/源码地图和 AI 改动审查清单。

一个用于验证“运行中的软件生成候选后继版本”的 Rust-first 通用语言原型。当前
实现还是安全内核阶段；Reader、Parser、AST、解释器、Web 服务、
版本库、LLM provider、影子执行与运维工具都已有第一方 Rust 实现，并全局禁止
第一方 `unsafe`。冻结的旧前端只服务于 v1 差分验证，不再承接 v2 语言开发。

当前闭环：

```text
稳定程序 -> 运行测试/观察失败 -> LLM/provider 生成候选源码
         -> 受限解释执行 -> 完整回归测试 -> 注册 -> 晋升 -> 回滚
```

这里的 AI 是候选代码开发者，不是测试裁判。测试集和解释器由宿主持有；AI
不能修改测试、绕过解释器、给自己判定通过，或直接改变活动版本。

## 直接测试 Web 后端

```powershell
.\scripts\serve-tasks.ps1
```

看到 `"event":"listening"` 后打开 <http://127.0.0.1:8080/>。页面不是静态
演示：新增、编辑、完成和删除任务都会经过 `.ail` 路由、受限解释器和事务
JSON KV；关闭后再次启动，任务仍然存在。端口可在启动前通过
`$env:AI_EVOLVE_HTTP_PORT="9000"` 修改。

启动脚本会先运行 [11 个有状态业务场景](examples/tasks/scenarios.json)，通过后
才把 [任务服务](examples/tasks/service.ail) 提升为活动版本。HTTP 请求在开始时
固定源码版本，因此运行中提升新版本不会改变正在处理的请求。

Rust HTTP 主机也已经可以独立完成测试门禁、部署和监听：

```powershell
# 终端 1
.\scripts\serve-tasks-rust.ps1

# 终端 2
Invoke-RestMethod http://127.0.0.1:8081/tasks
```

该入口当前提供 JSON API；浏览器测试控制台仍由上面的 Racket 启动脚本提供。
Rust 绑定地址可通过 `$env:AI_EVOLVE_RUST_HTTP_BIND="127.0.0.1:9001"` 修改。
Rust server 只接受 loopback 地址；公网入口必须经可信 TLS 反向代理。需要认证时在启动前
设置 `$env:AI_EVOLVE_HTTP_BEARER_TOKEN`，调用方使用 `Authorization: Bearer ...`。
Token 不会进入 guest headers、诊断或启动 JSON。每个响应都带独立的 `X-Request-Id`。
服务器同时把脱敏请求观测追加到 `<data-store>.observations.jsonl`：只包含时间、请求 ID、
方法、状态、耗时、handler、固定的活动版本哈希和错误码，不记录 URL/path、query、header
或 body。日志写入失败会作为独立运维事件报告，不会把已经提交的业务请求伪装成失败。

Rust server 还可以让一个尚未晋升的候选版本接收脱敏影子流量。它与活动版本读取同一份
请求前 KV 快照，但只在隔离内存中执行；候选写入、日志和响应全部丢弃，不影响主响应：

```powershell
$env:AI_EVOLVE_SHADOW_VERSION="<已注册候选的 64 位哈希>"
$env:AI_EVOLVE_SHADOW_PERCENT="10"
$env:AI_EVOLVE_SHADOW_MAX_CONCURRENCY="4"
.\scripts\serve-tasks-rust.ps1
```

采样由宿主请求 ID 确定，相同 ID 的决策稳定。结果写入
`<data-store>.shadow.jsonl`，只含活动/候选版本、状态、handler、错误码和差异类别，
不含 path、query、header、body、KV 值或内容指纹。候选缺失、被篡改或执行失败只会
产生 `candidate-unavailable`/差异观测；完整边界见
[影子运行](docs/shadow-rollout.md)。

## 快速开始

主实现只需要仓库声明的 Rust 工具链：

```powershell
.\scripts\check-rust.ps1
cargo run --locked -p ail-cli -- test-service examples\expenses\service.ail examples\expenses\scenarios.json
cargo run --locked -p ail-cli -- inspect-bundle examples\bundles\expense-approval
cargo run --locked -p ail-cli -- run-bundle examples\bundles\expense-approval evaluate examples\bundles\expense-approval\arguments.json
cargo run --locked -p ail-cli -- package-lock examples\packages\typed-expense .runtime\v0.9-package-store examples\packages\typed-expense\ail.lock.json
cargo run --locked -p ail-cli -- package-review .runtime\v0.9-package-store examples\packages\typed-expense\ail.lock.json --text
```

Rust conformance 固定 v1 兼容语义。需要额外运行冻结旧前端的差分证据时，显式启用；它会按需使用仓库私有工具链，不修改系统 PATH：

```powershell
$env:AI_EVOLVE_CHECK_V1_REFERENCE="1"
.\scripts\check-rust.ps1
.\scripts\bootstrap.ps1
.\scripts\test.ps1
.\scripts\demo.ps1
```

`bootstrap.ps1` 下载并校验官方 Racket 9.3 Windows x64 tarball。当前工作区
已经完成这一步，`.toolchains` 不进入版本控制。

演示会：

1. 将有缺陷的折扣程序作为初始稳定版；
2. 运行三个案例并观察两个失败；
3. 通过离线 provider 取得候选程序；
4. 验证候选程序的全部案例；
5. 晋升候选版本，VIP 价格由 `100` 变为 `90`；
6. 回滚到父版本，结果恢复为 `100`。

## CLI

```powershell
.\.toolchains\racket\Racket.exe src\cli.rkt check examples\discount\v2.ail
.\.toolchains\racket\Racket.exe src\cli.rkt inspect examples\discount\v2.ail
.\.toolchains\racket\Racket.exe src\cli.rkt test examples\discount\v2.ail examples\discount\tests.json
.\.toolchains\racket\Racket.exe src\cli.rkt run examples\discount\v2.ail calculate-discount examples\discount\vip-args.json
.\.toolchains\racket\Racket.exe src\cli.rkt test-service examples\tasks\service.ail examples\tasks\scenarios.json
.\.toolchains\racket\Racket.exe src\cli.rkt deploy-service examples\tasks\service.ail examples\tasks\scenarios.json .runtime\tasks\code
.\.toolchains\racket\Racket.exe src\cli.rkt serve-active .runtime\tasks\code 8080 .runtime\tasks\store.json

cargo run --locked -p ail-cli -- deploy-service examples\tasks\service.ail examples\tasks\scenarios.json .runtime\tasks-rust\code
cargo run --locked -p ail-server -- .runtime\tasks-rust\code 127.0.0.1:8081 .runtime\tasks-rust\store.json

# 停服后创建、校验并恢复到全新目标（命令不会覆盖已有路径）
cargo run --locked -p ail-cli -- backup-service .runtime\tasks-rust\code .runtime\tasks-rust\store.json .backups\tasks
cargo run --locked -p ail-cli -- verify-backup .backups\tasks
cargo run --locked -p ail-cli -- restore-service .backups\tasks .runtime\tasks-restored\code .runtime\tasks-restored\store.json
```

`run` 的最后一个参数既可以是 JSON 文本，也可以是 JSON 文件路径；Windows
下推荐文件路径，避免 shell 转义差异。所有正常结果、测试报告和语言错误都
使用 JSON 输出。

## 接入 LLM

实时 provider 支持 OpenAI Responses API，以及 DeepSeek 的 OpenAI-compatible
Chat Completions API。密钥只从进程环境读取；程序不会自动读取 `.env`，也不会
把密钥写入提示、诊断或版本元数据。

```powershell
# DeepSeek V4 Flash：安全提示输入密钥，通过测试才提升
.\scripts\live-demo.ps1

# 只生成、解释和测试候选，不提升（需先配置环境密钥）
.\.toolchains\racket\Racket.exe src\cli.rkt evolve examples\discount\v1.ail examples\discount\tests.json
```

可配置项见 [.env.example](.env.example) 和
[docs/live-provider.md](docs/live-provider.md)。候选即使生成成功也不会自动提升；
`--promote` 只是宿主侧请求，测试不通过时活动版本保持不变。DeepSeek 适配器
使用 `/chat/completions` 和 JSON Output，参见
[DeepSeek Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
与 [JSON Output 指南](https://api-docs.deepseek.com/guides/json_mode/)。OpenAI
Responses API 的请求结构参见
[OpenAI 官方迁移指南](https://developers.openai.com/api/docs/guides/migrate-to-responses)，
严格输出结构参见
[Structured Outputs 指南](https://developers.openai.com/api/docs/guides/structured-outputs)。

Rust host 也已接入相同的活动服务演化门禁。先通过环境变量配置 provider，再运行：

```powershell
# 默认只生成、测试并注册候选，不提升
cargo run --locked -p ail-cli -- evolve-service `
  .runtime\tasks-rust\code `
  examples\tasks\scenarios.json

# 只有 11 个场景全通过时才会响应显式提升请求
cargo run --locked -p ail-cli -- evolve-service `
  .runtime\tasks-rust\code `
  examples\tasks\scenarios.json `
  --promote
```

Rust transport 强制 HTTPS、拒绝重定向、限制请求/响应大小和墙钟时间，并在释放时清零
内存中的 API key。当前进程没有配置 provider 环境变量，因此本次迁移只执行了模拟响应
测试；真实调用仍必须由操作者在进程环境中提供凭据。

## 语言示例

```lisp
(program
  (name discount)
  (version 1)
  (capabilities)
  (def calculate-discount
    (fn (price user-type)
      (if (= user-type "vip")
          (- price (quotient price 10))
          price)))
  (export calculate-discount))
```

v1 支持 `quote`、`if`、顺序 `let`、`fn`、`do` 和函数调用，以及整数、
字符串、布尔、符号、列表、Map、`Ok`/`Err`。v2 增加真正短路的
`and/or`、穷尽式 `cond`、`number->string`、有界 `list-map/list-filter/list-fold/sum`、
`enum/union` Schema、带 fuel 成本的 `validate-report`，以及只返回业务 Result 的
`checked-quotient/checked-remainder`。完整示例见
[费用审批服务](examples/expenses/service.ail)及其[五个场景](examples/expenses/scenarios.json)。

v3 增加显式 `imports`、用户定义的封闭数据类型、带绑定的 `match`，以及由模块
SHA-256 汇成根哈希的密封 Bundle。带 imports 的单文件不能直接执行；宿主必须先验证
完整依赖图并链接命名空间。可运行示例见
[多模块费用审批 Bundle](examples/bundles/expense-approval)，完整契约见
[v0.7 规格](docs/spec-v0.7.md)。

v4 要求导出函数签名和 typed data field，并用独立的 `export-types` 声明跨模块名义类型；运行前推断内部类型并静态计算每个 export
的传递 capability 闭包。Bundle format v2 把闭包封进根 hash，加载时重新计算比对；
宿主输入与 guest 输出也按签名检查。`review` / `review-bundle` 可生成带 source span
和 effect 的 Rust 风格只读审查视图。示例见
[typed-expense Bundle](examples/bundles/typed-expense)，契约见
[v0.8 规格](docs/spec-v0.8.md)。

v0.9 增加内容寻址 package store 与精确 `ail.lock.json`：开发路径只在打包时使用，锁定加载只读取
`store/sha256/<hash>` 并重新验证 manifest、源码、依赖、类型和 capability 闭包。`ail-library`
同时把 `text@1` 迁到可替换 Rust Backend；contract 在进入 backend 前校验类型并扣除 fuel，guest
不能选择 crate、provider、动态库或任意宿主函数。完整示例见
[typed-expense package](examples/packages/typed-expense)，契约见
[v0.9 规格](docs/spec-v0.9.md)。

Web 程序可声明静态 `route`，处理器接收请求 Map 并返回结构化响应 Map。所有版本
都只有 `#f` 为假。

业务输入可以声明为编译器持有的 Schema：

```lisp
(schema task-create
  (object
    (required "id" (string 1 64))
    (required "title" (string 1 120))
    (optional "completed" boolean #f)))
```

`validate` 返回 `Ok` 或包含稳定问题列表的 `Err`；Schema 会拒绝额外字段、
补入默认值并消耗解释器 fuel。`api-response` 和 `api-error` 用于生成统一 HTTP
响应。完整语义见
[Business Backend Specification v0.3](docs/business-backend-v0.3.md)。

纯标准库通过版本化契约声明，宿主决定使用 Racket、Rust 或其他一致实现：

```lisp
(libraries (text 1))
(text/replace value "AI" "machine")
```

`text@1` 还提供字符计数、前缀、后缀和包含判断。契约拥有函数集合、类型和
fuel 成本；后端不能增加隐藏函数或改变版本。可运行案例在
[examples/libraries](examples/libraries)，完整边界见
[Library Backend Specification v0.4](docs/library-backend-v0.4.md)。

解释执行具有 fuel 和调用深度限制。客体默认没有文件、网络或数据库访问；
`log`、事务 `kv` 和 `clock` 都必须显式声明并由宿主注入。

## 目录

```text
docs/                    v0.1 提案、设计与语言规格
src/reader.rkt           安全读取和源码规模限制
src/parser.rkt           S 表达式到独立 AST
src/runtime.rkt          受限树遍历解释器
src/schema.rkt           有边界的声明式业务数据校验
src/library-contract.rkt 版本化标准库函数、类型和成本契约
src/library-backend.rkt  Racket 参考后端；未来由 Rust/Python/WASM 实现
src/test-suite.rkt       JSON 回归测试协议
src/service.rkt          路由匹配、响应契约和能力注入
src/kv-store.rkt         内存/文件事务 JSON KV
src/http-server.rkt      有边界的本地 HTTP/1.1 JSON 服务
src/service-test-suite.rkt  有状态 Web 业务场景测试
src/service-deployment.rkt  测试门禁、活动版本加载与演化
src/version-store.rkt    SHA-256 版本、晋升、回滚、审计事件
src/http-json.rkt        有超时、大小限制和结构化错误的 HTTPS JSON 传输
src/evolver.rkt          离线、Responses API 与 DeepSeek Chat provider
src/evolution-loop.rkt   生成、验证、注册和可选提升的一步闭环
src/cli.rkt              JSON CLI 和端到端演示
examples/discount/       初始版本、候选版本和测试案例
examples/tasks/          完整任务 CRUD 服务与业务场景
examples/libraries/      text@1 后端无关示例与测试
web/tasks/               同源响应式测试控制台
tests/all.rkt             宿主无关语义的起始一致性测试
```

## Rust 迁移原则

Rust 版本必须复用 `.ail` 源码、JSON 测试、诊断代码、版本文件和一致性测试。
只重写 Reader、AST、解释器、资源限制与能力分发。Racket 专属宏、任意宿主
调用和裸 `eval` 都不属于语言语义。

当前 Rust host 已迁移 Reader、Parser、解释器、Schema、Library Backend、服务能力、
事务/文件 KV、版本库、活动版本 HTTP API、认证/脱敏观测、隔离影子运行、离线备份恢复，以及 OpenAI/DeepSeek Provider。运行 `./scripts/check-rust.ps1` 会同时执行第一方 unsafe
门禁、Rust 测试、Clippy，以及语言、任务服务和版本生命周期的 Racket/Rust 精确差分。
默认网页服务尚未切换到 Rust，Provider 的真实联网烟雾测试需要操作者配置环境凭据。

## 当前安全边界

这是可用于本地业务原型的概念验证，不是公网生产服务器。Rust HTTP 已有 Bearer
认证、请求身份、脱敏 JSONL 观测、连接并发、读取/正文/响应限制和响应头校验，解释器已有
fuel 和调用深度限制；生产版仍需细粒度授权、TLS/反向代理、独立 OS 进程、数据库适配器、
正式数据库/PITR、异地备份、日志轮转/采集、告警，以及审批和灰度门禁。影子运行不是灰度：
候选永远不服务用户响应，尚不能作为生产流量切换机制。文件后端的离线
快照、逐文件校验与拒绝覆盖恢复已经可用，见 [备份与恢复](docs/backup-restore.md)。
