# CLI 参考

所有命令从仓库根目录运行。为便于复制，下面先定义可执行文件变量：

```powershell
$ail = ".\.toolchains\racket\Racket.exe"
```

CLI 正常结果、测试报告和已知语言错误都输出 JSON。命令分发的真实实现见 [src/cli.rkt](/source/src/cli.rkt.txt)。

## 语言与纯函数

### `check` / `inspect`

```powershell
& $ail src\cli.rkt check examples\discount\v2.ail
& $ail src\cli.rkt inspect examples\discount\v2.ail
```

两者当前行为相同：读取、解析并输出结构化 program / AST。适合检查 Parser 看见的内容。

### `run`

```powershell
& $ail src\cli.rkt run `
  examples\discount\v2.ail `
  calculate-discount `
  examples\discount\vip-args.json
```

格式：

```text
run <program.ail> <exported-entry> <args-json-or-file>
```

最后一个参数必须表示 JSON 数组；Windows 推荐传文件路径，避免 shell 引号差异。

### `test`

```powershell
& $ail src\cli.rkt test `
  examples\discount\v2.ail `
  examples\discount\tests.json
```

运行纯函数 JSON suite。只要有失败，进程 exit code 为 1。

### `conformance`

```powershell
& $ail src\cli.rkt conformance conformance\v1\manifest.json
```

运行与宿主无关的语言一致性语料。目前 Racket 是语义 oracle；迁移期间 Rust runner
必须读取同一份 manifest，并与其中的 canonical value / diagnostic 完全相等。任一 case
不匹配时进程 exit code 为 1。格式说明见
[conformance-v1.md](/source/docs/conformance-v1.md.txt)。

### `demo`

```powershell
& $ail src\cli.rkt demo
```

离线演示：从有缺陷折扣程序开始，通过 file provider 取得已知候选，测试、注册、晋升、调用，再回滚到父版本。它不请求真实 LLM。

## Web 服务

### `test-service`

```powershell
& $ail src\cli.rkt test-service `
  examples\tasks\service.ail `
  examples\tasks\scenarios.json
```

在内存 KV 和固定时钟上顺序运行有状态业务场景，不监听端口。

### `serve`

```powershell
& $ail src\cli.rkt serve `
  examples\tasks\service.ail `
  8080 `
  .runtime\tasks\store.json
```

格式：

```text
serve <program.ail> <port> <data-store.json>
```

每个请求重新加载指定源码文件，因此适合本地编辑观察；没有代码版本门禁。数据仍写入 file KV。

### `deploy-service`

```powershell
& $ail src\cli.rkt deploy-service `
  examples\tasks\service.ail `
  examples\tasks\scenarios.json `
  .runtime\tasks\code
```

先跑完整 service suite。全通过才把 candidate 注册并提升为活动版本；报告不通过时返回 exit code 1，active 不变。

### `serve-active`

```powershell
& $ail src\cli.rkt serve-active `
  .runtime\tasks\code `
  8080 `
  .runtime\tasks\store.json
```

格式：

```text
serve-active <code-store> <port> <data-store.json>
```

每个请求从 code store 读取一次 active source 并固定到请求结束。CLI 同时把脱敏观察追加到 `<data-store>.observations.jsonl`。

### `rollback-service`

```powershell
& $ail src\cli.rkt rollback-service .runtime\tasks\code
```

把 active 指回当前版本 metadata 中的 parent。没有活动版本或没有 parent 时返回结构化版本错误。

## AI 演化

### `evolve`

```powershell
& $ail src\cli.rkt evolve `
  examples\discount\v1.ail `
  examples\discount\tests.json
```

生成并测试纯函数候选，默认不晋升。显式请求晋升：

```powershell
& $ail src\cli.rkt evolve `
  examples\discount\v1.ail `
  examples\discount\tests.json `
  --promote
```

`--promote` 不是绕过测试；失败候选仍不能成为 active。

### `evolve-service`

```powershell
& $ail src\cli.rkt evolve-service `
  .runtime\tasks\code `
  examples\tasks\scenarios.json
```

它以当前 active service 源码为起点，让 provider 提出候选，再运行有状态场景。也支持最后加 `--promote`。

## Provider 环境变量

| 变量 | 用途 |
| --- | --- |
| `AI_EVOLVE_PROVIDER` | `openai-responses` 或 `deepseek-chat` |
| `AI_EVOLVE_API_KEY` | 通用 Bearer key，也可用 provider 专用变量 |
| `AI_EVOLVE_BASE_URL` | API base URL |
| `AI_EVOLVE_MODEL` | 模型 ID |
| `AI_EVOLVE_REASONING_EFFORT` | reasoning effort |
| `AI_EVOLVE_MAX_OUTPUT_TOKENS` | 候选输出上限 |
| `AI_EVOLVE_TIMEOUT_SECONDS` | provider 请求超时 |
| `AI_EVOLVE_STORE` | 纯函数 evolve 的版本库路径覆盖 |

不要把 key 写进命令历史或仓库。项目自带 `scripts/live-demo.ps1` 会以安全提示方式读取 DeepSeek key。

## Exit code

| code | 含义 |
| --- | --- |
| 0 | 操作成功 / suite 全通过 |
| 1 | 已知语言、provider、测试或版本操作失败 |
| 2 | CLI 参数不符合任何命令格式 |

自动化脚本应同时检查 exit code 和 JSON 中的 `ok` 字段。

## Rust 迁移期 CLI

当前 Rust host 已支持前端、语言/服务一致性语料、版本库生命周期、测试门禁部署和
活动版本 HTTP API：

```powershell
cargo run --quiet --locked -p ail-cli -- inspect examples\tasks\service.ail
cargo run --quiet --locked -p ail-cli -- conformance conformance\v1\manifest.json
cargo run --quiet --locked -p ail-cli -- test-service `
  examples\tasks\service.ail `
  examples\tasks\scenarios.json
cargo run --quiet --locked -p ail-cli -- deploy-service `
  examples\tasks\service.ail `
  examples\tasks\scenarios.json `
  .runtime\tasks-rust\code
cargo run --quiet --locked -p ail-server -- `
  .runtime\tasks-rust\code `
  127.0.0.1:8081 `
  .runtime\tasks-rust\store.json
cargo run --quiet --locked -p ail-cli -- version-conformance `
  examples\discount\v1.ail `
  examples\discount\v2.ail
```

`scripts/check-rust.ps1` 会把上述结果与 Racket canonical JSON 做精确差分。迁移期仍以
Racket 网页服务作为默认宿主；Rust 已能安全切换活动版本并承接 JSON HTTP 监听。
也可以直接运行 [serve-tasks-rust.ps1](/source/scripts/serve-tasks-rust.ps1.txt)。
