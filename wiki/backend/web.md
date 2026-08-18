# Web 后端与路由

`.ail` 只声明业务路由和 handler；socket、HTTP 解析、JSON、超时、事务和版本选择都在宿主侧。这个分工类似 Go/Rust Web 框架把 transport adapter 与业务 service 分开。

## 声明路由

```lisp
(route GET "/tasks" list-tasks)
(route POST "/tasks" create-task)
(route GET "/tasks/:id" get-task)
(route PUT "/tasks/:id" update-task)
(route DELETE "/tasks/:id" delete-task)
```

支持 GET、POST、PUT、PATCH、DELETE。`:id` 捕获一个已解码的路径段。相同 method 下重复或可能重叠的模式会在 Parser 阶段拒绝，不等到线上请求才猜测优先级。

每个 handler 必须同时存在于 `def` 和 `export` 中。

## Handler 收到什么

每个路由函数只接收一个不可变 request Map：

```json
{
  "method": "GET",
  "path": "/tasks/42",
  "params": {"id": "42"},
  "query": {"limit": "10"},
  "headers": {"content-type": "application/json"},
  "body": null
}
```

读取路径参数：

```lisp
(get (get request "params") "id")
```

读取 JSON body：

```lisp
(get request "body")
```

Go 类比是一个固定 DTO：

```go
type ServiceRequest struct {
    Method  string
    Path    string
    Params  map[string]string
    Query   map[string]string
    Headers map[string]string
    Body    Value
}
```

## Handler 必须返回什么

返回值必须恰好包含 `status`、`headers`、`body`：

```json
{
  "status": 200,
  "headers": {"content-type": "application/json"},
  "body": {"id": "42", "title": "读 Wiki"}
}
```

通常不要手写 Map，而使用：

```lisp
(api-response 200 task)
(api-error 404 "TASK_NOT_FOUND" "task does not exist")
```

宿主会验证 status 范围、header 名称和值、换行注入风险，以及 body 能否安全编码成 JSON。非法响应转换成结构化 500，同时丢弃 KV 事务。

## 完整读取 Handler

```lisp
(def get-task
  (fn (request)
    (let ((id (get (get request "params") "id"))
          (task (kv-get (string-append "task/" id) #f)))
      (if task
          (api-response 200 task)
          (api-error 404 "TASK_NOT_FOUND" "task does not exist")))))
```

逐行翻译：

1. 从 `request.params.id` 取 ID；
2. 组成 `task/<id>` 存储 key；
3. 读取 KV，找不到时返回 `#f`；
4. 找到则 200 返回任务；
5. 否则返回稳定错误码 `TASK_NOT_FOUND`。

真实版本：[examples/tasks/service.ail](/source/examples/tasks/service.ail.txt)。

## 每个请求一个事务

[kv-store.rkt](/source/src/kv-store.rkt.txt) 会在锁内复制当前数据，handler 只操作 working set：

```text
store snapshot → working copy → handler
                                │
                 ┌──────────────┴──────────────┐
                 │合法 response，无 diagnostic│异常 / 超时 / 非法 response
                 ▼                             ▼
               commit                       discard
```

因此下面的代码即使先执行了 `kv-put`，只要后面产生解释器错误，也不会提交：

```lisp
(do
  (kv-put key value)
  (get (map) "missing")) ; RUNTIME_MISSING_KEY，整笔请求回滚
```

当前文件 adapter 原子替换 JSON 文件，适合本地原型。未来 PostgreSQL adapter 应保持同样的 capability 和提交语义。

## HTTP host 的限制

[http-server.rkt](/source/src/http-server.rkt.txt) 当前提供：

- 只监听配置的 host，演示默认 `127.0.0.1`；
- 固定 worker 并发上限；
- request line、header 总量、header 数、body 大小限制；
- 请求读取 deadline 与 handler 墙钟超时；
- 解释器 fuel 和调用深度限制；
- 仅非流式 JSON，一个连接一个请求，`Connection: close`；
- 请求开始时固定活动程序版本；
- 对外隐藏内部诊断，只暴露 request ID。

Rust 迁移路径已经使用 [ail-http](/source/rust/crates/ail-http/src/lib.rs.txt) 的
Axum/Tokio HTTP/1.1 adapter，并由
[ail-server](/source/rust/crates/ail-server/src/main.rs.txt) 提供独立进程入口。它保留目标、
header、body、响应和并发上限，使用成熟协议栈处理连接，并在每个请求开始时从兼容版本库
加载一次活动源码。真实 loopback TCP 测试覆盖监听、请求、解释执行、响应和优雅关闭。

Rust server 只允许 loopback 监听，公网部署必须放在可信 TLS 反向代理后面。设置
`AI_EVOLVE_HTTP_BEARER_TOKEN` 可启用 Bearer 认证；宿主保存 token 摘要并做常量时间比较，
`authorization`、`cookie`、`proxy-authorization`、`x-api-key` 和客户端伪造的
`x-request-id` 不会传给 `.ail` handler。
每个响应都有 `X-Request-Id`，内部错误公开正文中的 ID 与宿主诊断使用同一个值。

Rust host 当前把同步解释器放到 blocking worker，并以解释器 fuel/depth 作为 guest 的硬
执行边界。它没有伪造一个无法取消 blocking 写事务的 handler 墙钟超时；生产隔离阶段需要
把不可取消工作放进可终止的独立进程，之后才能安全提供强墙钟 deadline。

## 状态码行为

| 情况 | 状态码 |
| --- | --- |
| 路由不存在 | 404 |
| path 存在但 method 不允许 | 405，并返回 `Allow` |
| 非 JSON request body | 415 |
| 缺少或错误的 Bearer token | 401，并返回 `WWW-Authenticate: Bearer` |
| HTTP / JSON 格式错误 | 400 |
| 请求或 header 超限 | 413 |
| handler 超时 | 504 |
| 客体诊断或非法 response | 500 + public request ID |

业务 400、404、409 等由 `.ail` handler 使用 `api-error` 返回。

## 当前边界

这个服务器适合语义验证和本地业务原型，不适合直接暴露公网。生产版至少还需要：

- 反向代理、TLS、细粒度身份/角色授权；
- 独立 OS 进程或更强沙箱；
- PostgreSQL 等正式数据库、连接池、迁移与备份；
- 内存和响应输出上限的完整治理；
- 指标、trace、告警、审计、灰度和人工审批；
- Rust 静态资源交付或独立前端/反向代理部署。

输入校验和错误契约见 [Schema 与统一错误](/backend/schema-errors)。
