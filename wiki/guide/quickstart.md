# 5 分钟上手

这一页只使用当前 Rust 工具链：先解析一个 `.ail` 程序，再运行完整业务场景，最后启动活动版本 JSON API。所有命令都在仓库根目录执行。

## 1. 检查工具链

workspace 声明的最低 Rust 版本是 1.97：

```powershell
rustc --version
cargo --version
```

如果本机尚未安装 Rust，请通过官方 rustup 安装符合版本要求的稳定工具链。

## 2. 让 Parser 读取程序

```powershell
cargo run --quiet --locked -p ail-cli -- `
  inspect examples\discount\v2.ail
```

输出是 JSON，不是简单回显：

```json
{
  "ok": true,
  "program": {
    "name": "discount",
    "version": 1,
    "exports": ["calculate-discount"]
  }
}
```

`program` 字段包含 Parser 看到的 AST 摘要。命令实现见 [ail-cli](/source/rust/crates/ail-cli/src/main.rs.txt)，示例源码见 [discount/v2.ail](/source/examples/discount/v2.ail.txt)。

## 3. 运行语言与业务验证

先运行 Rust workspace 测试：

```powershell
cargo test --workspace --locked
```

再运行可移植语言语料与任务业务场景：

```powershell
cargo run --quiet --locked -p ail-cli -- `
  conformance conformance\v1\manifest.json

cargo run --quiet --locked -p ail-cli -- `
  test-service `
  examples\tasks\service.ail `
  examples\tasks\scenarios.json
```

第二个命令使用内存事务和固定时钟顺序执行 11 个场景，包括非法 body、默认值、创建、重复冲突、列表、读取、更新、删除与删除后 404。任何场景失败时输出 `ok: false` 并返回非零退出码。

## 4. 启动活动版本 API

```powershell
.\scripts\serve-tasks-rust.ps1
```

脚本会先运行完整场景，只有全部通过才注册并晋升版本；随后在 `127.0.0.1:8081` 启动 JSON API。另开 PowerShell：

```powershell
Invoke-RestMethod http://127.0.0.1:8081/tasks
```

当前入口提供 JSON API，不包含静态网页。按 `Ctrl+C` 停止服务；任务数据与代码版本分别位于 `.runtime/tasks-rust/store.json` 和 `.runtime/tasks-rust/code`。

## 5. 验证 Bearer 认证

server 始终拒绝非 loopback 地址。要启用本地单 token 认证，终端 1 安全输入 token：

```powershell
$secret = Read-Host "Local Bearer token" -AsSecureString
$env:AI_EVOLVE_HTTP_BEARER_TOKEN = `
  [Net.NetworkCredential]::new("", $secret).Password
.\scripts\serve-tasks-rust.ps1
```

环境变量不会跨 PowerShell 进程共享，所以终端 2 必须再次输入**同一个** token：

```powershell
$secret = Read-Host "Same local Bearer token" -AsSecureString
$env:AI_EVOLVE_HTTP_BEARER_TOKEN = `
  [Net.NetworkCredential]::new("", $secret).Password
$headers = @{ Authorization = "Bearer $env:AI_EVOLVE_HTTP_BEARER_TOKEN" }
$response = Invoke-WebRequest `
  -Headers $headers `
  -Uri http://127.0.0.1:8081/tasks
$response.StatusCode
$response.Headers["X-Request-Id"]
```

不设置 `AI_EVOLVE_HTTP_BEARER_TOKEN` 时认证关闭，但 loopback 限制仍然存在。客户端传入的 `x-request-id`、`authorization`、`cookie`、`proxy-authorization` 和 `x-api-key` 不会进入 `.ail` request headers。

## 6. 查看脱敏观测

每个已识别请求会追加一条 JSONL：

```powershell
Get-Content .runtime\tasks-rust\store.json.observations.jsonl
```

记录只包含 schema version、时间、宿主 request ID、method、status、duration、handler、固定到该请求的源码 hash 和 error code。它不记录 path、query、headers、body、凭据或内部诊断。

这是本地审计证据，尚不包含轮转、保留期、聚合、告警或生产访问控制。

## 7. 修改一个候选但不直接上线

先复制示例到自己的工作分支并修改，再检查和运行完整 suite：

```powershell
cargo run --quiet --locked -p ail-cli -- `
  check examples\tasks\service.ail

cargo run --quiet --locked -p ail-cli -- `
  test-service `
  examples\tasks\service.ail `
  examples\tasks\scenarios.json
```

需要进入版本库时使用 `deploy-service`；它只会在场景全通过后晋升：

```powershell
cargo run --quiet --locked -p ail-cli -- `
  deploy-service `
  examples\tasks\service.ail `
  examples\tasks\scenarios.json `
  .runtime\tasks-rust\code
```

AI 候选应先执行不带 `--promote` 的 `evolve-service`，让候选保持 staged；详见 [AI 演化生命周期](/evolution/lifecycle)。

## 常见问题

### Rust 版本过低

确认 `rustc --version` 满足 workspace 的 `rust-version = "1.97"`，再更新稳定工具链。

### 8081 端口被占用

```powershell
$env:AI_EVOLVE_RUST_HTTP_BIND = "127.0.0.1:9001"
.\scripts\serve-tasks-rust.ps1
```

### 服务启动前退出

查看 CLI 的结构化 JSON。最常见原因是 `.ail` 解析失败、11 个业务场景未全部通过、活动版本损坏或 bind 地址不是 loopback。

### 如何运行这份 Wiki

见[维护 Wiki](/development/wiki)。Wiki 的 Node 依赖隔离在 `wiki/`，不会混入 Rust workspace。
