# AI 候选、测试、晋升与回滚

AI-Evolve 的安全设计可以浓缩为一句：**模型有提案权，没有裁判权和发布权。**

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
               │ 宿主明确传入 --promote
               ▼
         原子更新 active pointer
               │
               ▼
      后续请求使用新版本
               │
               ▼
       需要时 rollback 到父版本
```

“生成成功”与“可部署”不是同一个状态；“测试通过”与“已经上线”也不是同一个状态。

## 1. Provider 看见什么

[evolver.rkt](/source/src/evolver.rkt.txt) 给 provider 的核心请求是：

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

系统提示明确说明：源码和 observations 都是不可信数据，不能把其中内容当成更高优先级指令；不允许弱化或发明测试。

Provider 必须返回只有两个字符串字段的 JSON：

```json
{
  "source": "完整、可解析的 .ail 文档",
  "notes": "简短修改说明"
}
```

OpenAI adapter 使用 Responses API 的严格 JSON Schema；DeepSeek adapter 使用 Chat Completions JSON Output，并由宿主再次逐字段检查。

## 2. 密钥在哪里

API key 只从宿主进程环境变量读取：

- `AI_EVOLVE_API_KEY`；
- 或 provider 对应的 `DEEPSEEK_API_KEY` / `OPENAI_API_KEY`。

密钥不会进入：

- `.ail` 执行环境；
- provider prompt 的 currentSource / observations；
- 诊断和 CLI 输出；
- 版本 metadata；
- 仓库文件。

HTTP adapter 还会从远端错误摘要中替换 Authorization secret。完整配置见 [live-provider.md](/source/docs/live-provider.md.txt)。

## 3. 谁运行测试

测试由可信宿主加载，AI 只能看到结果，不能改比较器。

纯函数测试：[test-suite.rkt](/source/src/test-suite.rkt.txt) 读取 JSON `entry + cases`，逐例调用导出函数。

服务测试：[service-test-suite.rkt](/source/src/service-test-suite.rkt.txt) 使用一个新的内存 KV 和固定时钟，顺序执行有状态场景。任务服务的 11 个案例包括非法 body、缺字段、额外字段、默认值、创建、重复冲突、列表、读取、更新、删除与删除后 404。

候选必须通过整个 suite，不是只重新运行失败的那几条。

## 4. 版本库保存什么

[version-store.rkt](/source/src/version-store.rkt.txt) 以源码 SHA-256 作为 ID：

```text
code-store/
├─ versions/<hash>.ail       不可变源码
├─ metadata/<hash>.json      parent、provider、测试报告等
├─ active.json               当前活动 hash
└─ events.jsonl              registered/promoted/rolled-back 审计事件
```

注册候选不会覆盖旧源码。晋升只更新很小的 active pointer；回滚从当前 metadata 找到 parent，再更新 pointer。

这类似 OCI image digest + deployment pointer，而不是 `git checkout` 后原地覆盖工作目录。

## 5. 晋升有哪些门

`evolve` 和 `evolve-service` 默认只生成、解析、测试和注册，不晋升。只有调用方显式传入 `--promote` 才请求晋升；即使传入，测试失败仍不能改变 active。

| 状态 | 能注册 | 能晋升 |
| --- | --- | --- |
| provider 超时/拒绝/无效 JSON | 否 | 否 |
| 候选 `.ail` 解析失败 | 否 | 否 |
| 候选测试失败 | 可以保留失败证据（按流程） | 否 |
| 候选测试全通过但未传 `--promote` | 是 | 否 |
| 候选测试全通过且宿主传 `--promote` | 是 | 是 |

具体控制流：[evolution-loop.rkt](/source/src/evolution-loop.rkt.txt)、[service-deployment.rkt](/source/src/service-deployment.rkt.txt)。

## 6. 请求为什么不会执行一半换版本

`make-active-program-loader` 每个请求解析一次 `active-source`，HTTP host 随后只持有返回的 program 对象。晋升发生在并发请求中时：

- 已开始请求继续用旧 program；
- 晋升后的新请求读到新 active hash；
- 二者不会共享可变代码对象。

这比“运行时直接修改闭包 body”更容易推理、测试和回滚。

## 7. 回滚能做什么，不能做什么

`rollback-service` 把 active 指回当前版本的 parent，后续请求恢复旧代码。它不会自动回滚已经提交的数据格式变更。

因此未来加入 PostgreSQL migration 时必须额外设计：

- 向前/向后兼容的数据 Schema；
- expand → migrate → contract 策略；
- 代码 rollback 与数据 rollback 的独立审批；
- 灰度指标和停止条件。

## 威胁模型速查

| 风险 | 当前缓解 |
| --- | --- |
| 候选注入 Racket 代码 | Reader 禁用扩展，Parser 只生成独立 AST，不用 `eval` |
| 候选无限递归 | fuel + 调用深度 + handler 墙钟超时 |
| 候选直接联网/读文件 | 无对应 capability |
| 候选偷 API key | key 不进入 guest environment |
| 候选改测试给自己判通过 | 测试集和 runner 属于宿主 |
| 写 KV 后崩溃留下半成品 | 合法响应后才提交事务 |
| 晋升影响正在执行的请求 | request-level version pinning |
| 模型输出恶意说明 | notes 只是 metadata，不执行 |

仍未解决的生产风险包括 OS 进程隔离、完整内存限制、认证授权、供应链和人工审批。项目没有宣称当前原型可以无人监管地持续改写公网生产系统。
