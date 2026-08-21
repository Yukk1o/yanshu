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
| definition | 跳到同文件全局 `def`，或参数、顺序 `let`、pattern binding 的精确声明 |
| references | 查找同文件全局/局部变量引用，遵守 `includeDeclaration` 与词法遮蔽 |
| formatting | 返回 canonical full-document `TextEdit[]`；server 不写文件 |
| `yanshu/reviewDocument` | experimental 版本化请求；返回 `rust-readonly-v3`、`editable:false` 与审查文本 |

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
- 单次 references 最多 1,024 个 Location，超限返回 `LSP_REFERENCE_LIMIT`，不静默截断；
- review 输入最多 512 KiB、投影文本最多 4 MiB，版本和 renderer/read-only 契约必须精确匹配；
- 只接受 ASCII header 和 UTF-8 JSON body。

framing 缺失、重复 `Content-Length`、超限 header/body 或截断 body 会终止 server，而不是尝试猜测流边界。

review 请求只消费 `didOpen` / `didChange` 保存的指定版本快照。它调用正式 Parser、类型/效果分析和单向审查 renderer，不读取 URI 文件、不执行 guest、不返回 edit；六倍 JSON 转义上界仍小于 32 MiB 消息限制。版本漂移与 `LSP_REVIEW_*_LIMIT` 都失败关闭。

## 名称怎样解析

definition 与 references 不使用文本搜索。正式 AST 决定全局 `def`、函数、顺序 `let`、match arm 和嵌套遮蔽的解析；同源 Reader datum 只提供 `def`、参数与 `let` 名称的精确 span，pattern binding 使用 Parser 已保存的 span。声明和引用总量受 Reader 节点上限约束，AST 与源码 span 对不上时失败关闭。

因此，内层参数或 binding 与全局 `def` 同名时会解析到内层声明；顺序 `let` 的右侧只能引用更早的 binding；退出嵌套函数或 match arm 后会恢复外层作用域。全局定义的 signature、route handler 和 export 位置计入语义引用；字符串、注释、quote、类型和 Schema 名称不会冒充变量引用。references 按源码顺序返回当前打开文档中的 Location，并用一次向前扫描转换 UTF-16 range，避免结果数量增加时反复扫描源码。这个只读索引不改变 `.yan`、Bundle、package 或运行语义。

## 当前限制

- 没有局部增量同步；
- 没有 completion、rename、semantic tokens、code action；
- 没有 Tree-sitter grammar、Neovim 安装包或 macOS/Arm Extension Host 平台验收；
- 没有跨 Bundle/package 的多文件链接导航。

实现入口：[符号索引](/source/rust/crates/yanshu-syntax/src/symbol.rs.txt)、[协议 framing](/source/rust/crates/yanshu-lsp/src/protocol.rs.txt)、[文档与导航](/source/rust/crates/yanshu-lsp/src/document.rs.txt)、[server 生命周期](/source/rust/crates/yanshu-lsp/src/server.rs.txt)。完整契约见 [v0.12 规格](/source/docs/specs/v0.12.md.txt)。
