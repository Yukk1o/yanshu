# 给 Codex、Claude Code 与 OpenCode 使用的 MCP

`yanshu-mcp` 把正式 Reader、Parser、类型/效果分析、formatter 和 Rust 风格审查投影暴露成三个只读工具。Agent 先用自己的文件读取能力取得当前 `.yan` 文本，再把完整快照传给工具；server 本身不接收路径，也不读取或写入工作区。

这和 [AI Agent Backend](/development/ai-agents) 是两条互补路径：Agent Backend 让宿主启动 Agent 去编辑一次性候选目录；MCP 则让已经在仓库里工作的 Codex、Claude Code 或 OpenCode 随时调用 Yanshu 的权威语言工具。

## 构建

```powershell
cargo build --locked --release -p yanshu-mcp
```

Windows 产物是 `target\release\yanshu-mcp.exe`，Linux 产物是 `target/release/yanshu-mcp`。配置 Agent 时建议使用绝对路径，避免启动目录改变后找不到 server。

## 三个工具

| 工具 | 返回 | 不会做什么 |
| --- | --- | --- |
| `yanshu.inspect_source` | AST inspection；v4 还返回类型、效果与 capability 闭包 | 不执行 guest，不读取路径 |
| `yanshu.format_source` | `formattedSource`、`changed`、formatter 版本 | 不覆盖文件，不跳过重解析与语义复核 |
| `yanshu.review_source` | 机器分析和 `rust-readonly-v3` 审查文档 | 不把审查文本当 Rust 或 `.yan` 输入 |

每个工具都声明 `readOnlyHint: true`、`destructiveHint: false`、`idempotentHint: true` 和 `openWorldHint: false`。成功与语言诊断都同时返回 `structuredContent` 和兼容的 JSON 文本；未知工具或畸形 JSON-RPC 是协议错误，Parser、类型或 formatter 失败是 `isError: true` 的可修复工具结果。

## 连接 Codex

按 [Codex 官方 MCP 文档](https://developers.openai.com/codex/mcp/) 可以直接用 CLI 添加本地 stdio server：

```powershell
codex mcp add yanshu -- E:\learn\yanshu\target\release\yanshu-mcp.exe
codex mcp list
```

也可以写入用户级或可信项目级 `config.toml`：

```toml
[mcp_servers.yanshu]
command = "E:\\learn\\yanshu\\target\\release\\yanshu-mcp.exe"
enabled_tools = [
  "yanshu.inspect_source",
  "yanshu.format_source",
  "yanshu.review_source",
]
```

Codex CLI、IDE 扩展和同一宿主上的 ChatGPT 桌面端共享这份 MCP 配置。连接后可用 `/mcp` 查看状态。

## 连接 Claude Code

按 [Claude Code 官方 MCP 文档](https://code.claude.com/docs/en/mcp) 添加本地、当前项目私有的 stdio server：

```powershell
claude mcp add --transport stdio --scope local yanshu -- `
  E:\learn\yanshu\target\release\yanshu-mcp.exe
claude mcp list
```

需要与团队共享时可以改用 `--scope project`，但不要提交只在一台机器成立的绝对路径；应先约定安装位置或用文档化的环境变量展开。Claude Code 中使用 `/mcp` 查看连接和工具数。

## 连接 OpenCode

当前 [OpenCode v2 官方文档](https://opencode.ai/v2/docs/mcp-servers) 把本地 server 放在 `mcp.servers` 下。`opencode.json` 示例：

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "yanshu": {
        "type": "local",
        "command": [
          "E:\\learn\\yanshu\\target\\release\\yanshu-mcp.exe"
        ]
      }
    }
  }
}
```

运行 `opencode mcp list` 检查状态。OpenCode v1 与 v2 配置形状不同，不要把旧版顶层 `mcp.<name>` 示例混进 v2 配置。

## 建议给 Agent 的用法

可以直接要求：

```text
读取 policy.yan，把当前完整文本交给 yanshu.inspect_source。
若有诊断，依据稳定错误码修复；再调用 yanshu.format_source，
展示 diff 后应用 formattedSource，最后用 yanshu.review_source 复核效果和 capability。
```

MCP 不会替 Agent 应用格式化结果。小改动仍可由人直接编辑 `.yan`，随后调用工具复核；不必每次都让模型重新生成整份程序。

## 协议与资源边界

- stdio 只输出一行一个 UTF-8 JSON-RPC 对象，stdout 不混入日志；
- 同时支持 MCP `2026-07-28` 的 `server/discover` 无握手协议，以及 `2025-11-25`、`2025-06-18`、`2025-03-26`、`2024-11-05` 的 `initialize` 兼容路径；
- 单条输入最多 4 MiB，单条输出最多 32 MiB；
- 工具源码最多 512 KiB，仍受 Reader 的 token、节点数和深度限制；
- formatter 输出最多 512 KiB，审查文本最多 4 MiB，结构化 payload 最多 8 MiB；
- 调用顺序串行，因此同一进程没有并发分析风暴；
- server 不读环境凭据、不访问文件或网络、不调用 LLM/provider、不运行 guest，也不提供写工具。

512 KiB 是 Agent 工具入口的主动收紧，不改变 CLI/Reader 对规范源码的 4 MiB 总上限。输入 JSON 最坏六倍转义和输出的结构化/文本兼容副本都有编译期上界，并在序列化后再次检查实际字节数。

实现入口：[stdio framing](/source/rust/crates/yanshu-mcp/src/protocol.rs.txt)、[协议分派](/source/rust/crates/yanshu-mcp/src/server.rs.txt)、[只读工具](/source/rust/crates/yanshu-mcp/src/tools.rs.txt)。完整契约见 [v0.12 规格](/source/docs/specs/v0.12.md.txt)。
