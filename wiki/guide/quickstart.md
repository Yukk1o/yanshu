# 5 分钟上手

这一页只做两件事：先跑测试，再打开真实任务后端。命令均在仓库根目录执行。

## 1. 准备 Racket 工具链

```powershell
.\scripts\bootstrap.ps1
```

脚本下载官方 Minimal Racket 9.3 Windows x64 压缩包、核对 SHA-256，再放进项目私有的 `.toolchains/`。它不会修改系统 PATH。已经存在工具链时只打印版本。

脚本源码：[scripts/bootstrap.ps1](/source/scripts/bootstrap.ps1.txt)。

## 2. 运行全部测试

```powershell
.\scripts\test.ps1
```

成功时会看到 Racket 测试汇总。测试覆盖语言、Schema、事务 KV、HTTP 边界、版本门禁、候选演化和任务业务场景。测试入口：[tests/all.rkt](/source/tests/all.rkt.txt)。

## 3. 启动任务服务

```powershell
.\scripts\serve-tasks.ps1
```

启动脚本会先：

1. 解析 [service.ail](/source/examples/tasks/service.ail.txt)；
2. 运行 [11 个有状态场景](/source/examples/tasks/scenarios.json.txt)；
3. 只有全通过才注册并晋升这个版本；
4. 从活动版本启动 HTTP 服务。

出现类似下面的 JSON 后，打开 <http://127.0.0.1:8080/>：

```json
{
  "ok": true,
  "event": "listening",
  "host": "127.0.0.1",
  "port": 8080
}
```

网页中的新增、编辑、完成和删除都会经过 `.ail` 路由与事务 KV，不是前端假数据。按 `Ctrl+C` 停止服务，再启动后任务仍会从 `.runtime/tasks/store.json` 恢复。

::: tip 修改端口
```powershell
$env:AI_EVOLVE_HTTP_PORT = "9000"
.\scripts\serve-tasks.ps1
```
随后打开 `http://127.0.0.1:9000/`。
:::

## 4. 不经过网页调用 API

保持服务运行，另开 PowerShell：

```powershell
$body = @{
  id = "read-wiki"
  title = "读懂 AI-Evolve"
} | ConvertTo-Json

Invoke-RestMethod `
  -Method Post `
  -Uri http://127.0.0.1:8080/tasks `
  -ContentType application/json `
  -Body $body

Invoke-RestMethod -Uri http://127.0.0.1:8080/tasks
```

`completed` 没有传入时，Schema 会补成 `false`；`createdAt` 和 `updatedAt` 来自宿主注入的 `clock` 能力。

## 5. 看懂第一个命令

直接检查一个纯函数程序：

```powershell
.\.toolchains\racket\Racket.exe src\cli.rkt `
  check examples\discount\v2.ail
```

输出的 `program` 不是简单回显，而是 Parser 生成的结构化 AST。这相当于：

```go
source := readFile("v2.ail")
program, err := Parse(Read(source))
json.NewEncoder(os.Stdout).Encode(program)
```

或者 Rust：

```rust
let source = fs::read_to_string("v2.ail")?;
let program: Program = parse(read(&source)?)?;
println!("{}", serde_json::to_string(&program)?);
```

下一步建议阅读[架构导览](/guide/architecture)，不需要先啃完整 Lisp 语法。

## 常见问题

### `Racket.exe` 不存在

重新运行 `scripts/bootstrap.ps1`。网络下载失败时，检查是否能访问 Racket 官方下载地址；不要把 `.toolchains/` 提交到 Git。

### 8080 端口被占用

按上面的方式设置 `AI_EVOLVE_HTTP_PORT`，例如 9000。

### 服务启动前就退出

查看 CLI 返回的结构化错误。最常见原因是 `.ail` 解析失败或 11 个业务场景未全部通过；这是部署门禁在工作，而不是 HTTP 服务器随机崩溃。

### 如何运行这份 Wiki

见[维护 Wiki](/development/wiki)。Wiki 使用独立 `wiki/package.json`，不会把 Node 依赖混进 Racket 工具链。
