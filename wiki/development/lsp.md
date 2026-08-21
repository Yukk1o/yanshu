# 最小 LSP Server

`yanshu-lsp` 把 Parser、类型/效果分析、稳定节点路径和 formatter 接到支持 Language Server Protocol 的编辑器。它遵循[官方 LSP 3.18 规范](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.18/specification/)，通过 stdio 通信。

## 启动命令

先构建：

```powershell
cargo build --locked -p yanshu-lsp
```

编辑器的 language server command 指向：

```text
target/debug/yanshu-lsp.exe   # Windows
target/debug/yanshu-lsp       # Linux
```

server 不监听端口，不读取 workspace 文件，也不访问网络。编辑器必须用 `didOpen` / `didChange` 发送完整 `.yan` 文本。

VS Code 用户可以直接使用[平台专用扩展](/development/vscode)，不必手工配置 protocol client。

## 当前能力

| LSP 能力 | 当前行为 |
| --- | --- |
| position encoding | 明确协商 `utf-16`，中文和非 BMP 字符不会错位 |
| document sync | open/close + full change；version 必须递增 |
| diagnostics | Parser 错误；language v4 还包含类型与 effect/capability 错误 |
| hover | 当前 definition 的类型、effects 和最深 `expression-v1` 节点路径 |
| definition | 跳到同文件全局 `def` 名称；局部遮蔽时返回空，不会误跳 |
| formatting | 返回 canonical full-document `TextEdit[]`；server 不写文件 |

格式化 edit 由编辑器展示和应用。`.yan` 原文仍是规范输入，LSP 不能执行 Rust 审查视图，也不能替 Parser、测试、fuel 或内容哈希做决定。

## 诊断如何产生

每次 open/change 都从编辑器快照重新运行正式 Reader/Parser。解析失败时发布稳定 `READ_*` / `PARSE_*` code、UTF-16 range 和人类消息；解析成功且是 language v4 时继续运行类型与效果分析。

通知本身没有 JSON-RPC response。若 change 版本倒退、携带增量 `range`、超过大小或形状不合法，server 不修改现有快照，只发送不含源码和 URI 的 `window/logMessage`。

## 有界协议

- JSON-RPC body 最多 32 MiB；
- header 总计最多 16 KiB、32 行，单行最多 8 KiB；
- 单份源码最多 4 MiB；
- 最多 32 个打开文档，总源码最多 16 MiB；
- URI 最多 4 KiB；
- 只接受 ASCII header 和 UTF-8 JSON body。

framing 缺失、重复 `Content-Length`、超限 header/body 或截断 body 会终止 server，而不是尝试猜测流边界。

## 当前限制

- 没有局部增量同步；
- 局部参数、`let` 和 pattern binding 只用于防止误跳，尚不能跳到声明；
- 没有 completion、references、rename、semantic tokens、code action；
- 没有 Tree-sitter grammar、Neovim 安装包或 VS Code Extension Host 端到端测试；
- 没有跨 Bundle/package 的多文件链接导航。

实现入口：[协议 framing](/source/rust/crates/yanshu-lsp/src/protocol.rs.txt)、[文档与导航](/source/rust/crates/yanshu-lsp/src/document.rs.txt)、[server 生命周期](/source/rust/crates/yanshu-lsp/src/server.rs.txt)。完整契约见 [v0.12 规格](/source/docs/specs/v0.12.md.txt)。
