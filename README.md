# AI-Evolve

一个用于验证“运行中的软件生成候选后继版本”的小型原型。宿主暂时使用
Racket，客体程序使用项目自己的受限 Lisp；解释器不会把客体代码交给
Racket `eval`。

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

## 快速开始

项目使用私有 Minimal Racket 工具链，不修改系统 PATH。

```powershell
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

目前支持 `quote`、`if`、顺序 `let`、`fn`、`do` 和函数调用，以及整数、
字符串、布尔、符号、列表、Map、`Ok`/`Err`。Web 程序可声明静态
`route`，处理器接收请求 Map 并返回结构化响应 Map。只有 `#f` 为假。

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

解释执行具有 fuel 和调用深度限制。客体默认没有文件、网络或数据库访问；
`log`、事务 `kv` 和 `clock` 都必须显式声明并由宿主注入。

## 目录

```text
docs/                    v0.1 提案、设计与语言规格
src/reader.rkt           安全读取和源码规模限制
src/parser.rkt           S 表达式到独立 AST
src/runtime.rkt          受限树遍历解释器
src/schema.rkt           有边界的声明式业务数据校验
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
web/tasks/               同源响应式测试控制台
tests/all.rkt             宿主无关语义的起始一致性测试
```

## Rust 迁移原则

Rust 版本必须复用 `.ail` 源码、JSON 测试、诊断代码、版本文件和一致性测试。
只重写 Reader、AST、解释器、资源限制与能力分发。Racket 专属宏、任意宿主
调用和裸 `eval` 都不属于语言语义。

## 当前安全边界

这是可用于本地业务原型的概念验证，不是公网生产服务器。HTTP 已有连接并发、
读取、正文、执行时限和响应头校验，解释器已有 fuel 和调用深度限制；生产版
仍需认证授权、TLS/反向代理、独立 OS 进程、数据库适配器、内存/输出限制、
备份迁移，以及审批、灰度和运行期指标。
