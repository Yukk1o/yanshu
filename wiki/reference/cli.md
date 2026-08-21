# CLI 参考

所有命令从仓库根目录运行。当前 CLI 是 Rust workspace 中的 `yanshu-cli`，正常结果、验证报告和已知错误都输出 JSON。

下面写出完整 `cargo run`，便于直接复制。命令分发见 [yanshu-cli main.rs](/source/rust/crates/yanshu-cli/src/main.rs.txt)。

## `check` / `inspect`

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  check examples\discount\v2.yan

cargo run --quiet --locked -p yanshu-cli -- `
  inspect examples\discount\v2.yan
```

两者当前行为相同：读取、解析并输出结构化 Program / AST 摘要。适合确认 Parser 实际看到了哪些 definition、Schema、route、capability、library 和 export。

格式：

```text
check <program.yan>
inspect <program.yan>
```

## 内容寻址包与锁文件

```powershell
$workspace = "examples\packages\typed-expense"
$store = ".runtime\v0.9-package-store"

cargo run --quiet --locked -p yanshu-cli -- `
  package-lock $workspace $store "$workspace\yanshu.lock.json"

cargo run --quiet --locked -p yanshu-cli -- `
  package-review $store "$workspace\yanshu.lock.json" --text

cargo run --quiet --locked -p yanshu-cli -- `
  package-run $store "$workspace\yanshu.lock.json" evaluate "$workspace\arguments.json"
```

完整命令：

```text
package-pack <workspace> <store>
package-lock <workspace> <store> <yanshu.lock.json>
package-verify <store> <content-hash>
package-inspect <store> <yanshu.lock.json>
package-review <store> <yanshu.lock.json> [--text]
package-run <store> <yanshu.lock.json> <export> <arguments.json>
```

`package-lock` 递归打包根 workspace 内的源码依赖，将 artifact 写入 `store/sha256/<hash>`，再生成规范 lock。inspect/review/run 只读取 lock 与 store，不读取开发依赖路径。`--text` 只改变审查展示格式；不加时保留 JSON。

## fuel 字节码与 WASM

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  package-compile $store "$workspace\yanshu.lock.json" `
  .runtime\typed-expense.ybc.json `
  .runtime\typed-expense.wasm

cargo run --quiet --locked -p yanshu-cli -- `
  package-run-compiled $store "$workspace\yanshu.lock.json" `
  .runtime\typed-expense.wasm evaluate "$workspace\arguments.json"
```

单文件命令：

```text
compile-bytecode <program.yan> <artifact.ybc.json>
inspect-bytecode <program.yan> <artifact.ybc.json>
run-bytecode <program.yan> <artifact.ybc.json> <export> <arguments.json>
compile-wasm <program.yan> <artifact.wasm>
inspect-wasm <program.yan> <artifact.wasm>
run-wasm <program.yan> <artifact.wasm> <export> <arguments.json>
```

锁定 package 命令：

```text
package-compile <store> <yanshu.lock.json> <artifact.ybc.json> <artifact.wasm>
package-run-compiled <store> <yanshu.lock.json> <artifact.wasm> <export> <arguments.json>
```

密封 Bundle 也可直接编译：

```text
compile-bundle <directory> <artifact.ybc.json> <artifact.wasm>
run-bundle-compiled <directory> <artifact.wasm> <export> <arguments.json>
```

编译前重跑类型/效果分析；加载时要求 artifact 是给定 Program/lock 的规范编译结果。正常运行返回 `fuelLimit / fuelConsumed / fuelRemaining`。CLI 只显式提供 `log` adapter，并以 `logEvents` 返回本次事件数量；它不注入 KV、clock 或网络能力，也不把日志值打印到终端。WASM 目标使用显式 `yanshu_v1.execute` handle ABI，详情见 [fuel 字节码与 WASM](/language/bytecode-wasm)。

## 密封、检查与运行 Bundle

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  seal-bundle examples\bundles\expense-approval expense-app app.yan policy.yan

cargo run --quiet --locked -p yanshu-cli -- `
  inspect-bundle examples\bundles\expense-approval

cargo run --quiet --locked -p yanshu-cli -- `
  review-bundle examples\bundles\typed-expense

cargo run --quiet --locked -p yanshu-cli -- `
  run-bundle examples\bundles\expense-approval evaluate examples\bundles\expense-approval\arguments.json
```

格式：

```text
seal-bundle <directory> <entry> <module.yan>...
inspect-bundle <directory>
review-bundle <directory>
run-bundle <directory> <export> <arguments.json>
```

`seal-bundle` 解析全部模块、验证依赖图，并写入 name-sorted `bundle.json`；返回的 `bundleHash` 是规范 manifest 的 SHA-256。`inspect-bundle` 和 `run-bundle` 都会重新读取并校验每个 module hash，不信任已有 manifest。参数文件必须是 JSON 数组。

`review-bundle` 对完整链接程序运行类型/效果分析，再返回 `rust-readonly-v3` 文本和带 source span 的 machine-readable nodes。它不会写回源码。v3 会把 `Int` 的任意精度、Yanshu truthiness 和 capability effect 调用直接标在文本中；单文件也可以使用 `review <program.yan>`，两者末尾加 `--text` 可直接打印带缩进文本。

## `conformance`

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  conformance conformance\v1\manifest.json
```

运行语言一致性语料，验证合法程序、portable value、Schema、library 和稳定 diagnostic。仓库内 `conformance/v1` 到 `v4` 还覆盖条件/集合/Result、用户数据与 match、密封 Bundle、类型及字节码执行。任一 case 不匹配时 JSON 为 `ok: false`，进程返回非零退出码。

## `test-service`

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  test-service `
  examples\tasks\service.yan `
  examples\tasks\scenarios.json
```

格式：

```text
test-service <program.yan> <scenarios.json>
```

使用新的内存 KV 和固定时钟顺序运行有状态业务场景，不监听端口，也不修改版本库。

## `deploy-service`

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  deploy-service `
  examples\tasks\service.yan `
  examples\tasks\scenarios.json `
  .runtime\tasks-rust\code
```

格式：

```text
deploy-service <program.yan> <scenarios.json> <code-store>
```

执行顺序：解析 → 完整 service suite → 注册内容哈希版本 → 只有全通过才晋升。失败报告不会改变 active；已经是 active 的同内容源码不会重复晋升。

## `evolve-service`

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  evolve-service `
  .runtime\tasks-rust\code `
  examples\tasks\scenarios.json
```

格式：

```text
evolve-service <code-store> <scenarios.json> [--task <task.md>] [--promote]
```

命令从 active 版本读取当前源码和报告，让配置的 provider 提出完整候选，然后重新解析并运行完整 suite。

- 不带 `--promote`：通过测试的候选可以注册，但 active 不变；
- 带 `--promote`：只有候选全通过且不是当前版本才更新 active；
- `notes` 只进入不可信 metadata，不能代替测试或审查。

推荐先 staged，再由独立审查步骤决定是否执行带 `--promote` 的命令。

provider 可以是远程 OpenAI/DeepSeek HTTP adapter，也可以是本机已登录的 Codex、Claude Code 或 OpenCode。Agent Backend 只在一次性目录中交给工具 `candidate.yan`、结构化失败报告和语言速查，不暴露真实 suite、code store 或 active 指针；配置与边界见 [AI Agent Backend](/development/ai-agents)。

`--task` 提供最多 64 KiB 的 UTF-8 目标文件，让 agent 知道要新增或修复什么；目标与 observations 一样是不可信输入。选项顺序固定为 `--task <task.md> --promote`，推荐先不带 `--promote` 生成并审查候选。

## `version-conformance`

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  version-conformance `
  examples\discount\v1.yan `
  examples\discount\v2.yan
```

格式：

```text
version-conformance <initial.yan> <candidate.yan>
```

在临时版本库中验证内容哈希、注册、晋升、active 指针、重启读取、事件顺序与回滚生命周期。它是实现检查，不是操作现有生产 store 的回滚命令。

## 离线备份、校验与恢复

创建快照前先停止对应 server；快照目录必须不存在：

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  backup-service `
  .runtime\tasks-rust\code `
  .runtime\tasks-rust\store.json `
  .backups\tasks-2026-08-18
```

在传输后或恢复前重复做只读校验：

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  verify-backup .backups\tasks-2026-08-18
```

恢复目标 code store 和 data store 都必须不存在；命令没有覆盖开关：

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  restore-service `
  .backups\tasks-2026-08-18 `
  .runtime\tasks-restored\code `
  .runtime\tasks-restored\store.json
```

格式：

```text
backup-service <code-store> <data-store.json> <snapshot-dir>
verify-backup <snapshot-dir>
restore-service <snapshot-dir> <code-store> <data-store.json>
```

`backup-service` 先完成 VersionStore pending journal，再在离线 service lock 和版本库锁内创建 schema v1 manifest，对每个 payload 文件记录相对路径、大小和 SHA-256。`verify-backup` 还检查源码哈希、metadata、active、事件 sequence/hash chain、晋升报告与 KV v1 语义，并拒绝而不执行快照中的 journal；路径穿越、符号链接、未知/重复/超限文件和 hash/size 不一致都会失败。运行中的 server 持有 `<data-store>.service.lock`，维护命令会返回 `SERVICE_MAINTENANCE_LOCKED`。

观测 JSONL 不属于业务恢复点，不进入快照。恢复后应先重新运行完整业务场景，再在新的 loopback 端口验证；详细边界见[备份与恢复说明](/source/docs/backup-restore.md.txt)。

## 启动 HTTP server

先确保 code store 有活动版本，再启动独立 server：

```powershell
cargo run --quiet --locked -p yanshu-server -- `
  .runtime\tasks-rust\code `
  127.0.0.1:8081 `
  .runtime\tasks-rust\store.json
```

格式：

```text
yanshu-server <code-store> <loopback-bind-address> <data-store.json>
```

也可以使用 [serve-tasks-rust.ps1](/source/scripts/serve-tasks-rust.ps1.txt)，它会先验证并部署任务服务。

### Server 控制项

| 控制项 | 当前行为 |
| --- | --- |
| `YANSHU_HTTP_BEARER_TOKEN` | 可选；设置后每个请求必须使用同一 Bearer token |
| bind address | 只接受 IPv4 / IPv6 loopback；wildcard 与公网地址拒绝 |
| `X-Request-Id` | 宿主随机生成并写入所有已识别请求的响应 |
| guest headers | 过滤 `authorization`、`cookie`、`proxy-authorization`、`x-api-key`、`x-request-id` |
| program version | 请求开始时读取、验证并固定一次 active hash |
| observation | `<data-store>.observations.jsonl`，每请求一条有界脱敏记录 |
| `YANSHU_SHADOW_VERSION` | 可选；已注册候选的 64 位内容 hash |
| `YANSHU_SHADOW_PERCENT` | 启用影子时必填；整数 `1..100` |
| `YANSHU_SHADOW_MAX_CONCURRENCY` | 影子后台并发上限，默认 `4` |

启动 JSON 会给出 `authenticationRequired`、observation 路径、`shadowEnabled`、固定候选与影子观测路径。普通观测字段是 `schemaVersion/timestampMs/requestId/method/status/durationMs/handler/version/errorCode`；不记录 path、query、headers、body、凭据或内部诊断详情。

影子模式必须同时设置 `VERSION` 与 `PERCENT`。候选使用活动请求提交前的 KV 快照，但只在隔离内存执行；结果追加到 `<data-store>.shadow.jsonl`，不会改变真实 KV 或用户响应。记录只含版本、状态、handler、错误码和差异类别，不含内容值或内容指纹。配置与边界见[影子运行说明](/source/docs/shadow-rollout.md.txt)。

Bearer 只解决本地单 token 认证，不能替代 TLS、用户身份、角色授权或可信反向代理。

## Provider 环境变量

| 变量 | 用途 |
| --- | --- |
| `YANSHU_PROVIDER` | `openai-responses` / `deepseek-chat` / `codex-cli` / `claude-code-cli` / `opencode-cli`（含短别名） |
| `YANSHU_API_KEY` | 通用 key；也可使用 provider 专用变量 |
| `OPENAI_API_KEY` | OpenAI key；DeepSeek 模式也会作为兼容后备读取 |
| `DEEPSEEK_API_KEY` | DeepSeek key |
| `YANSHU_BASE_URL` | HTTPS API base URL |
| `YANSHU_MODEL` | 模型 ID |
| `YANSHU_REASONING_EFFORT` | reasoning effort |
| `YANSHU_MAX_OUTPUT_TOKENS` | 正整数候选输出上限 |
| `YANSHU_TIMEOUT_SECONDS` | 正整数请求超时秒数 |
| `YANSHU_AGENT_COMMAND` | 可选的 agent CLI 可执行文件名或绝对路径 |
| `YANSHU_AGENT_TIMEOUT_SECONDS` | agent 墙钟超时，默认 600 秒、最大 3600 秒 |

不要把 key 写进命令历史、Wiki 或仓库。HTTP provider 可用 `Read-Host -AsSecureString` 在当前进程临时设置；Agent Backend 则会过滤敏感环境变量，应使用 agent 自己的安全登录存储。

## JSON 与退出码

| 结果 | exit code |
| --- | --- |
| 命令成功且报告通过 | 0 |
| 已知语言、provider、suite、版本或参数错误 | 1 |

自动化应同时检查 exit code 和 JSON 顶层 `ok`。不要只搜索终端文本，因为稳定接口是 `code/message/details` 和结构化 report。
