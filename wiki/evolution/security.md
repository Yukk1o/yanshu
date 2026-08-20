# 安全模型与能力边界

Yanshu 假设 `.yan` 源码、LLM 候选、HTTP 输入和模型说明都不可信。可信部分是语言前端、解释器预算、能力 dispatcher、测试 runner、版本库和晋升策略。

## 信任边界

| 可信控制面 | 不可信输入 |
| --- | --- |
| Reader / Parser / AST 规则 | `.yan` 源码与 LLM 候选 |
| fuel、调用深度、Schema 与 HTTP 上限 | 候选的循环、递归和大型数据 |
| capability dispatcher | guest 请求的 KV、clock、log 调用 |
| 测试集与比较器 | provider notes 和候选自述 |
| 内容寻址版本库与 active 指针 | 候选请求“立即上线”的文本 |
| 密钥加载与 HTTPS provider adapter | current source、observations、远端响应 |

## 源码隔离

Reader 只接受语言定义的一个 S 表达式；Parser 只生成自己的 AST。未知 form、扩展读取语法、重复定义、非法 route 或未声明 library 在执行前就失败。宿主不把候选当作原生源码求值。

## 资源预算

```text
source node/depth limit
        ↓
parser structure limits
        ↓
fuel + call-depth budget
        ↓
schema issue / collection limits
        ↓
HTTP target / header / body / response limits
```

fuel 不只计算语法步骤，也计算值节点、标量字节、整数位数、集合/Schema 遍历和高成本 BigInt 操作。Reader 在解析 BigInt 前限制源码与 token；运行时对输入、常量、变量复制、返回值以及 capability/Library Backend 结果使用统一的深度、节点和字节包络。字符串拼接与 `text/replace` 输出放大在分配前拒绝，解释器与字节码 VM 具有相同耗尽边界。

这些结构性上限使受限执行不再只靠“循环计步”，但它仍不是操作系统级墙钟或地址空间隔离。当前同步任务运行在 blocking worker；生产版仍应使用可终止的独立进程或更强沙箱处理宿主缺陷和可信 Backend 故障。

## 能力默认拒绝

程序只有同时满足“源码声明 + 宿主注入”才能调用 capability。当前 guest 没有文件、网络、环境变量、系统命令或 provider 凭据访问权。

新增能力时至少要定义：

- 可调用操作与参数/结果类型；
- 单次和累计预算；
- 事务或幂等语义；
- 可记录与必须脱敏的字段；
- 测试 adapter；
- 生产授权和撤销方式。

## API key 与 provider

provider key 只从宿主环境读取，不进入 guest environment、prompt 中的源码/观察、诊断、版本 metadata 或仓库文件。HTTP provider adapter 只允许 HTTPS、拒绝 redirect、限制请求/响应大小与时间，并对密钥使用零化容器。

Codex、Claude Code 与 OpenCode Agent Backend 不继承名称包含 key/token/secret/password/credential 的环境变量。它们只在一次性候选目录中工作，进程有超时，输出限为普通有界 UTF-8 文件；宿主随后独立解析和测试。候选目录不是完整 OS 沙箱，高风险环境仍需容器或独立低权限账户，详见 [AI Agent Backend](/development/ai-agents)。

## HTTP 边界

当前 Rust server：

- 只允许 IPv4 / IPv6 loopback 监听；
- 可用 `YANSHU_HTTP_BEARER_TOKEN` 启用单 token Bearer 认证；
- 由宿主生成 request ID，不信任客户端 `x-request-id`；
- 不把认证、cookie、credential/secret token 与宿主 request ID 传给 guest；
- 拒绝 guest 设置 `content-length`、`transfer-encoding`、`connection`、`upgrade`、认证和 cookie 等宿主专属响应头；
- 每个请求读取并固定一次 active hash；
- 写入不含 path、query、headers、body 和诊断详情的有界 JSONL 观测。
- 可把候选放入有并发上限的影子执行；候选只读取请求前 KV 快照，全部写入与响应都会丢弃。

这些措施适合本地验证，不等于生产认证授权。Bearer 不是用户、角色或资源级权限；公网仍需要 TLS 反向代理、身份系统、进程隔离、数据库、备份、日志轮转、指标告警和灰度策略。

## AI 为什么不是裁判

模型可以读取当前源码和结构化观察并提交完整候选，但它不能：

- 修改可信测试 runner；
- 把 notes 当成通过证据；
- 跳过 Parser 或完整 suite；
- 直接写 active 指针；
- 读取 provider key；
- 给自己增加未注入 capability。

即使测试全部通过，是否晋升仍是独立策略决定；测试也不能替代业务意图审查。具体流程见[候选、验证、晋升与回滚](/evolution/lifecycle)。

## 备份与恢复边界

单机文件后端提供离线 `backup-service`、只读 `verify-backup` 和拒绝覆盖的 `restore-service`。server 在整个生命周期持有 service lock，备份同时持有版本库锁，避免活动指针、版本事件或 KV 在快照中间变化。恢复先写入不可见的同级暂存目标并完成语义校验，最后才提交；失败清理不会删除并发进程刚获得的版本锁。

manifest 逐文件记录 SHA-256 和大小，验证还会检查版本事件与 KV 语义；恢复目标必须不存在。快照不包含 provider 密钥、TLS/反向代理配置、操作系统权限或观测日志，也不替代加密、签名、异地复制和恢复演练。命令见 [CLI 参考](/reference/cli#离线备份校验与恢复)。

## 影子运行边界

配置候选 hash 与采样比例后，宿主使用自己生成的 request ID 做确定性分桶。被采样请求在活动版本提交前抓取 KV 快照，活动请求照常提交和返回；候选随后在后台只操作隔离内存。候选缺失、被篡改、执行失败或影子容量满都不能替换主响应。

`<data-store>.shadow.jsonl` 只持久化活动/候选版本、状态、handler、错误码和差异类别。请求与响应内容、header/KV 值以及内存中用于判等的摘要都不会落盘。当前 guest 没有外部 I/O capability；未来新增外部副作用能力时，必须先提供专用 shadow adapter。详见[影子运行说明](/source/docs/shadow-rollout.md.txt)。

## 当前威胁与状态

| 风险 | 当前缓解 | 仍需完成 |
| --- | --- | --- |
| 候选执行任意宿主代码 | 受限 AST 与自有解释器 | 继续扩大恶意语料 |
| 无限递归 | fuel + 调用深度 | 进程级内存/墙钟隔离 |
| 读取密钥或联网 | 无对应 capability | 新能力逐项审计 |
| 修改测试给自己放行 | runner 与 suite 在可信侧 | 测试变更审批与签名 |
| 请求执行中切换版本 | 每请求固定 active hash + 隔离 shadow | canary 与自动停止门禁 |
| 敏感请求进入观测 | 字段白名单 + 敏感 header 过滤 | 轮转、保留和访问控制 |
| 代码回滚但数据不兼容 | 不可变版本与 parent | 数据 migration 策略 |

上线前还应按[审查 AI 改动](/evolution/review-ai-change)逐项确认业务、Schema、路由、能力、错误和版本差异。
