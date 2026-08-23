# 让 Codex、Claude Code 与 OpenCode 理解 `.yan`

`yanshu-mcp` 是一个本地只读 MCP server。它让编程 Agent 调用 Yanshu 的正式 Parser、类型/效果分析、formatter 和 Rust 风格审查视图，而不是靠模型猜测 Lisp 语义。

Agent 先读取当前 `.yan` 文件，再把完整源码快照交给 MCP。server 本身不接收文件路径，也不读取或写入工作区。

## 安装 `yanshu-mcp`

从 [GitHub Release v0.12.0](https://github.com/Yukk1o/yanshu/releases/tag/v0.12.0) 下载与你平台匹配的 ZIP：

- Windows x86-64：`yanshu-v0.12.0-x86_64-pc-windows-msvc.zip`；
- Linux x86-64：`yanshu-v0.12.0-x86_64-unknown-linux-gnu.zip`。

v0.12.0 的每个平台 ZIP 都包含 `yanshu` CLI、`yanshu-mcp` 和 `yanshu-lsp`。下载后先验证来源，再解压：

```powershell
gh release download v0.12.0 --repo Yukk1o/yanshu --pattern "yanshu-v0.12.0-x86_64-pc-windows-msvc.zip"
gh attestation verify .\yanshu-v0.12.0-x86_64-pc-windows-msvc.zip --repo Yukk1o/yanshu
Expand-Archive .\yanshu-v0.12.0-x86_64-pc-windows-msvc.zip -DestinationPath C:\Tools
```

解压后的路径类似：

```text
C:\Tools\yanshu-v0.12.0-x86_64-pc-windows-msvc\yanshu-mcp.exe
```

Linux 使用同名的无 `.exe` 可执行文件。后面的配置都建议填写绝对路径。

发布清单使用 schema v3，并附带 CLI、MCP、LSP、VS Code 四份 CycloneDX SBOM。需要验证整套下载时，参见 [下载与验证 v0.12.0](/development/releases)。

如果你正在开发 Yanshu 本身，也可以从源码构建：

```powershell
cargo build --locked --release -p yanshu-mcp
```

## 连接 Codex

用 Codex CLI 注册本地 stdio server：

```powershell
codex mcp add yanshu -- C:\Tools\yanshu-v0.12.0-x86_64-pc-windows-msvc\yanshu-mcp.exe
codex mcp list
```

也可以在 Codex 的 `config.toml` 中配置：

```toml
[mcp_servers.yanshu]
command = "C:\\Tools\\yanshu-v0.12.0-x86_64-pc-windows-msvc\\yanshu-mcp.exe"
enabled_tools = [
  "yanshu.inspect_source",
  "yanshu.format_source",
  "yanshu.review_source",
]
```

连接后可以让 Codex：“读取 `policy.yan`，把完整文本交给 `yanshu.inspect_source`，根据诊断修改后再用 `yanshu.review_source` 复核 capability。”

## 连接 Claude Code

把 server 加到当前项目的本地配置：

```powershell
claude mcp add --transport stdio --scope local yanshu -- `
  C:\Tools\yanshu-v0.12.0-x86_64-pc-windows-msvc\yanshu-mcp.exe
claude mcp list
```

在 Claude Code 中可用 `/mcp` 查看连接状态。若团队要共享项目配置，不要提交只在某一台电脑成立的绝对路径；先约定安装目录。

## 连接 OpenCode

在 `opencode.json` 中注册本地 server：

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "yanshu": {
        "type": "local",
        "command": [
          "C:\\Tools\\yanshu-v0.12.0-x86_64-pc-windows-msvc\\yanshu-mcp.exe"
        ]
      }
    }
  }
}
```

运行 `opencode mcp list` 检查状态。不同 OpenCode 大版本的配置结构可能不同；若命令不能识别配置，请以你安装版本的官方文档为准。

## 三个可用工具

| 工具 | 用途 | 返回后由谁应用 |
| --- | --- | --- |
| `yanshu.inspect_source` | 解析源码，报告 AST、类型、效果与 capability 闭包 | Agent 根据稳定诊断修改源码 |
| `yanshu.format_source` | 生成经过重解析和语义复核的格式化源码 | Agent 或人审查 diff 后写回 |
| `yanshu.review_source` | 生成机器分析和 `rust-readonly-v3` 审查文档 | 人与 Agent 只读检查 |

MCP 不会替 Agent 保存文件。你也可以自己完成小改动，再让 Agent 调用这些工具复核，不必让模型每次重写整份程序。

## 必须知道的安全边界

`yanshu-mcp`：

- 不运行 guest 程序，也不调用其中的 capability；
- 不接收路径，不自行读取、创建、覆盖或删除工作区文件；
- 不访问网络、不调用 LLM/provider，也不读取环境凭据；
- 只返回源码分析、候选格式化文本和只读审查视图；
- 对输入、输出、源码和审查文本都有硬大小上限；
- 把格式化或语言错误作为结构化结果返回，不用宿主堆栈代替诊断。

因此 MCP 是 Agent 的语言工具，不是执行沙箱，也不是自动批准 AI 改动的机制。最终 `.yan` 仍需经过 Parser、类型/效果检查、fuel、测试和项目自己的审查流程。
