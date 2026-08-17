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
字符串、布尔、符号、列表、Map、`Ok`/`Err`。只有 `#f` 为假。

解释执行具有 fuel 和调用深度限制。客体默认没有文件、网络或数据库访问；
唯一的演示能力 `log` 也必须在程序中显式声明。

## 目录

```text
docs/                    v0.1 提案、设计与语言规格
src/reader.rkt           安全读取和源码规模限制
src/parser.rkt           S 表达式到独立 AST
src/runtime.rkt          受限树遍历解释器
src/test-suite.rkt       JSON 回归测试协议
src/version-store.rkt    SHA-256 版本、晋升、回滚、审计事件
src/http-json.rkt        有超时、大小限制和结构化错误的 HTTPS JSON 传输
src/evolver.rkt          离线、Responses API 与 DeepSeek Chat provider
src/evolution-loop.rkt   生成、验证、注册和可选提升的一步闭环
src/cli.rkt              JSON CLI 和端到端演示
examples/discount/       初始版本、候选版本和测试案例
tests/all.rkt             宿主无关语义的起始一致性测试
```

## Rust 迁移原则

Rust 版本必须复用 `.ail` 源码、JSON 测试、诊断代码、版本文件和一致性测试。
只重写 Reader、AST、解释器、资源限制与能力分发。Racket 专属宏、任意宿主
调用和裸 `eval` 都不属于语言语义。

## 当前安全边界

这是概念验证，不是生产沙箱。HTTP 已有限时和响应大小限制，解释器已有 fuel
和调用深度限制；生产版仍需把候选执行放入独立 OS 进程，增加内存和输出限制，
并通过审批、灰度和运行期指标决定是否提升。
