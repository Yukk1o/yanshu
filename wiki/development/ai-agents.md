# Codex、Claude Code 与 OpenCode Agent Backend

Yanshu 可以直接调用用户已经安装的 Codex、Claude Code 或 OpenCode，让它们修改一个 `.yan` 候选。这里的 agent 是“编写后端”，不是只会读仓库说明的聊天模型，也不是有权晋升代码的裁判。

## 实际调用链

```text
active 源码 + 当前失败报告
              ↓
一次性候选目录
  candidate.yan
  OBSERVATIONS.json
  TASK.md
  LANGUAGE.md
              ↓
Codex / Claude Code / OpenCode 编辑 candidate.yan
              ↓
Rust 宿主重新解析并运行可信 suite
              ↓
内容寻址登记 ── 人类显式请求后才可能晋升
```

真实 code store、测试文件、active 指针和生产 capability 都不放进候选目录。agent 退出成功不代表候选正确；Rust 宿主只接受它留下的有界 UTF-8 普通文件，随后重新走 Parser、测试和版本库门禁。

## 使用

先用对应工具自己的登录流程完成认证，然后选择后端：

```powershell
$env:YANSHU_PROVIDER = "codex-cli"

cargo run --locked -p yanshu-cli -- `
  evolve-service `
  .runtime\tasks\code `
  examples\tasks\scenarios.json `
  --task .\task.md
```

`task.md` 写本次新增或修复目标，例如“为 POST /tasks 增加标题重复检查并保持现有错误 envelope”。任务最多 64 KiB，只是交给 agent 的不可信目标，不会改变 suite 或安全规则。

另两个值是：

```powershell
$env:YANSHU_PROVIDER = "claude-code-cli"
$env:YANSHU_PROVIDER = "opencode-cli"
```

默认命令名分别是 `codex`、`claude`、`opencode`。如果可执行文件不在 PATH，可以显式设置：

```powershell
$env:YANSHU_AGENT_COMMAND = "C:\Tools\codex.exe"
$env:YANSHU_AGENT_TIMEOUT_SECONDS = "900"
```

`YANSHU_MODEL` 是可选的；不设置时使用 agent 自己的配置。不要在同一 shell 里依赖 API key 环境变量：Agent Backend 会移除名称含 key、token、secret、password 或 credential 的变量，工具应使用自己的安全凭据存储。

## 三种适配器

| 后端 | 非交互模式 | 受控工具面 |
| --- | --- | --- |
| Codex | `codex exec` | `workspace-write`，approval 为 never，关闭网络与 web search |
| Claude Code | `claude --print` | 只允许 Read/Edit/Write；拒绝 Bash、Web 与 Task |
| OpenCode | `opencode run` | 内联 permission 默认 deny，只开放候选文件读写，拒绝外部目录、bash、webfetch 与 task |

调用使用结构化参数，不经过 shell 拼接；有墙钟超时；candidate 和 notes 都有大小上限；symlink 输出会被拒绝；一次性目录在读取完成后清理。稳定失败码可区分命令不存在、非零退出、超时、输出缺失/超限/非法文件和候选未修改。

## 仍然不信任 Agent CLI

候选工作目录是在应用层缩小可见任务，不应冒充完整的 OS 安全边界。Codex/Claude Code/OpenCode 是运行在宿主侧的第三方开发工具，可能读取自己的用户配置，也可能随版本改变行为。高风险环境应再使用容器、虚拟机或独立低权限账户。

无论 agent 多聪明，它都不能：

- 修改真实测试或 code store；
- 直接获得 guest capability；
- 把 notes 或进程 exit code 当成验证结果；
- 绕过内容哈希、类型/效果、fuel 或 Parser；
- 在没有显式 `--promote` 且测试未通过时改变 active。

## 仓库开发支持

根目录另外提供 `AGENTS.md` 与 `CLAUDE.md`，它们共同引用 [`docs/ai-agent-guide.md`](/source/docs/ai-agent-guide.md.txt)。这是让 agent 修改语言实现时读取的项目契约，与上面的“一次性候选编写后端”互补，但不是同一件事。

当前已有 `.yan` LSP、平台专用 VS Code 扩展和只读 Tree-sitter grammar，支持诊断、hover、全文 semantic tokens、同文档 definition/references、防捕获 rename、格式化 edit、无脚本 Rust 风格只读审查面板，以及增量 CST/标准查询；还没有其它编辑器安装包、MCP server、semantic token range/delta 或审查视图结构化回写。Tree-sitter 只是显示层，不能替代正式 Parser 或 Agent 候选门禁。
