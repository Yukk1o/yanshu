# AI 候选、验证、晋升与回滚

AI-Evolve 的演化规则可以浓缩为一句：**模型有提案权，没有裁判权和发布权。**

## 完整生命周期

```text
活动源码 + 结构化观察
          │
          ▼
     LLM provider
          │ 完整候选 .ail + notes
          ▼
  Reader / Parser / AST
          │ 解析成功
          ▼
   完整回归 / 业务场景
          │
     ┌────┴────┐
     │失败     │全通过
     ▼         ▼
 记录报告   注册不可变候选（SHA-256）
               │
               │ 宿主明确请求 promote
               ▼
         原子更新 active pointer
               │
               ▼
      后续请求使用新版本
               │
               ▼
       需要时 rollback 到父版本
```

“生成成功”“测试通过”“已经晋升”是三个不同状态。

## 两层版本身份

每个候选同时有两个不能混用的字段：

| 身份 | 示例 | 回答的问题 |
| --- | --- | --- |
| 语言版本 | `(version 2)` / metadata `languageVersion: 2` | 这份程序按哪套 AST、Schema 和 primitive 语义解释？ |
| 内容身份 | 64 位 SHA-256 / metadata `hash` | 运行、测试或回滚的究竟是哪一份不可变源码？ |

语言版本相同不代表代码相同；内容 hash 相同则必须逐字节对应同一份 UTF-8 源码。当前 Parser 只接受已实现的 v1/v2，避免 LLM 写出一个未来版本号后让运行时自行猜测语义。

这正是给自动修复循环准备的协议：固定父 hash 与语言版本 → 消费结构化失败和成本 → 生成完整候选 → 得到新 hash → 重新跑同一门禁 → 注册、拒绝或晋升。循环不需要解析人类终端文本，也不能就地覆盖活动源码。

## 1. Provider 输入与输出

[ail-provider](/source/rust/crates/ail-provider/src/lib.rs.txt) 接收当前版本和结构化观察：

```json
{
  "currentHash": "...",
  "currentSource": "(program ...)",
  "observations": {
    "passed": false,
    "failures": []
  }
}
```

源码和 observations 都是不可信 prompt 数据，不能提升成系统指令。Provider 只能返回完整候选与说明：

```json
{
  "source": "完整、可解析的 .ail 文档",
  "notes": "简短修改说明"
}
```

OpenAI adapter 使用 Responses API 的严格 JSON Schema；DeepSeek adapter 使用 Chat Completions JSON Output。无论远端怎样约束，宿主都要再次验证字段、大小和候选语法。

## 2. 密钥留在宿主侧

API key 从 `AI_EVOLVE_API_KEY` 或 provider 专用环境变量读取。它不会进入：

- `.ail` 执行环境；
- prompt 的 current source / observations；
- 公共诊断和 CLI JSON；
- 版本 metadata；
- 仓库文件。

adapter 只接受 HTTPS、拒绝 redirect，并限制请求/响应大小与超时。配置变量见 [CLI Provider 配置](/reference/cli#provider-环境变量)。

## 3. 谁运行验证

测试由可信 runner 加载；模型可以看到报告，不能修改比较器。

- 语言 conformance 固定 portable value、AST 摘要和诊断行为；
- service suite 使用新的内存 KV 与固定时钟顺序执行有状态场景；
- 候选必须运行整个 suite，不能只重跑失败案例；
- 场景通过只说明符合现有断言，不证明业务意图完整。

service runner 见 [ail-service](/source/rust/crates/ail-service/src/lib.rs.txt)，任务 suite 见 [scenarios.json](/source/examples/tasks/scenarios.json.txt)。

## 4. 不可变版本库

[ail-store](/source/rust/crates/ail-store/src/lib.rs.txt) 以源码 SHA-256 作为 ID：

```text
code-store/
├─ versions/<hash>.ail       不可变源码
├─ metadata/<hash>.json      languageVersion、parent、provider、测试报告
├─ active.json               当前活动 hash
└─ events.jsonl              registered/promoted/rolled-back
```

注册不会覆盖旧源码。读取版本时还会重新校验 hash，防止路径替换或内容损坏。active 是很小的原子指针，不是工作目录里的可变源码。

同一语言版本的源码只要内容变化就生成新 hash；测试失败的候选仍可保留自己的报告与父链证据，但不能成为 active。于是“程序即数据，演化留痕，回滚廉价”是存储约束，不依赖模型自觉。

## 5. 晋升门禁

`evolve-service` 默认只生成、解析、测试和注册，不改变 active。只有显式 `--promote` 才请求晋升；失败报告仍然不能成为活动版本。

| 状态 | 能注册 | 能晋升 |
| --- | --- | --- |
| provider 超时、无效 JSON | 否 | 否 |
| 候选语法或结构非法 | 否 | 否 |
| 完整 suite 失败 | 保留报告取决于调用流程 | 否 |
| suite 全通过，未请求 promote | 是 | 否 |
| suite 全通过，显式请求 promote | 是 | 是 |

CLI 控制流见 [ail-cli](/source/rust/crates/ail-cli/src/main.rs.txt)。

## 6. 每个请求固定版本

HTTP program loader 在请求开始时完成一次：

```text
read active hash → verify source hash → parse Program → LoadedProgram
```

之后 route、handler 和 observation 都使用同一个 `LoadedProgram`：

- 已开始的请求继续使用旧版本；
- 晋升后的新请求读取新 active；
- 观测中的 `version` 是该请求真正执行的 hash；
- 不会执行到一半替换函数体。

实现见 [ail-http](/source/rust/crates/ail-http/src/lib.rs.txt)。

## 7. 晋升前影子运行

通过 suite 并已注册的候选可以在不晋升的情况下接收影子请求。宿主固定候选 hash，用 request ID 确定性采样，并在活动请求提交前复制 KV 状态。活动版本照常返回用户；候选只在有并发上限的后台内存 store 中执行，所有写入、日志和响应都会丢弃。

影子观测只回答“这次活动与候选在哪些可观测类别上不同”，不会自动晋升。候选不可用也只产生脱敏记录。完整隔离条件见[安全模型](/evolution/security#影子运行边界)。

## 8. 回滚边界

版本库可以把 active 指回当前 metadata 的 parent，让后续请求恢复旧代码。回滚不会自动撤销已经提交的数据变化。

因此正式数据库上线前必须设计：

- 向前/向后兼容的数据 Schema；
- expand → migrate → contract；
- 代码回滚与数据回滚的独立审批；
- shadow 累计阈值、canary 指标与停止条件。

当前 CLI 的 `version-conformance` 会验证注册、晋升和回滚生命周期；还没有面向生产操作者的完整回滚编排界面。

## 9. 审查仍是独立门

测试是必要条件，不是业务意图的替代品。晋升前应比较真实源码、场景、Schema、route、capability、错误 code 与数据兼容性，不要只读 provider notes。

详细清单见[如何审查 AI 生成的改动](/evolution/review-ai-change)，威胁模型见[安全模型](/evolution/security)。
