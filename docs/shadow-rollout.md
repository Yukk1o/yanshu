# 影子运行

Rust host 可以把已注册但尚未晋升的候选版本接到一部分真实请求上，同时保证候选既不能
修改真实数据，也不能改变用户响应。这是灰度发布前的观测门禁，不是流量切换。

## 启用

先通过 `evolve-service` 或 `deploy-service` 注册候选，保留输出中的 64 位内容哈希。启动
server 前同时设置：

```powershell
$env:YANSHU_SHADOW_VERSION="<candidate-hash>"
$env:YANSHU_SHADOW_PERCENT="10"          # 1..100
$env:YANSHU_SHADOW_MAX_CONCURRENCY="4"   # 可选，默认 4

.\scripts\serve-tasks-rust.ps1
```

`VERSION` 与 `PERCENT` 必须同时配置。格式错误、百分比越界、并发为零或影子日志无法打开
会拒绝启动，避免看似启用但实际没有观测。候选不存在或运行期完整性校验失败不会阻止活动
服务；被采样的请求会写入 `candidate-unavailable`。

## 执行顺序与隔离

对一个通过认证、协议解析且被采样的请求，宿主执行：

```text
固定活动版本 ─┐
               ├─ 锁内抓取请求前 KV 快照 ── 活动版本提交真实 KV ── 返回用户
固定候选哈希 ─┘                         └─ 后台候选读取隔离快照 ── 丢弃全部副作用
```

- 采样只使用宿主生成的 request ID 做 SHA-256 分桶，同一 ID 的结果确定；
- 活动版本和候选版本使用相同请求、时钟值与提交前 KV 状态；
- 候选仍受解释器 fuel、调用深度、Schema 和能力边界约束；
- 候选 `kv-put`/`kv-delete`、guest `log` 和响应不会进入真实服务；
- 后台任务有独立并发上限，满载时记录 `capacity-skipped`；
- 未认证请求、非法 JSON 和其他协议层失败不会送入候选；
- 候选加载、完整性或执行结果不会替换活动响应。

当前能力只有 `kv`、`clock` 和内存 `log`，没有外部网络、文件或任意 FFI；未来增加具有
外部副作用的能力时，必须先提供专门的 shadow adapter，不能直接复用生产 adapter。

## 脱敏观测

记录追加到 `<data-store>.shadow.jsonl`。每行固定包含：

- request ID、时间、活动版本与候选版本；
- `compared`、`candidate-unavailable` 或 `capacity-skipped`；
- 活动/候选的状态、handler、稳定错误码；
- `status`、`handler`、`error-code`、`headers`、`body` 差异类别。

请求 method/path/query/header/body、响应正文/响应头值、KV key/value、诊断详情均不落入
影子日志。响应头和正文只在内存中计算摘要以判等，摘要本身也会在写日志前丢弃，避免
对短值做字典枚举或跨请求关联。

## 当前门禁边界

影子相等只说明已采样输入上的可观测结果一致，不证明候选正确。当前还未实现：

- 累计样本数、错误率和差异率阈值；
- 时间窗口、告警和人工批准；
- 让候选真正服务少量用户的 canary；
- 自动晋升或自动回滚。

因此影子日志只能作为后续发布门禁的输入，永远不会自行晋升候选。
