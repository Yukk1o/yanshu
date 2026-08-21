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
| hover | 精确 token 的 form 语法、primitive/Library 合约、用户函数类型/effects、局部 binding 与稳定节点路径 |
| completion | 当前作用域可见的 form、binding、primitive、构造器、Schema、声明 Library operation 与类型；精确替换当前 token |
| definition | 跳到同文件全局 `def`，或参数、顺序 `let`、pattern binding 的精确声明 |
| references | 查找同文件全局/局部变量引用，遵守 `includeDeclaration` 与词法遮蔽 |
| formatting | 返回 canonical full-document `TextEdit[]`；server 不写文件 |
| `yanshu/reviewDocument` | experimental 版本化请求；返回 `rust-readonly-v3`、`editable:false` 与审查文本 |

格式化 edit 由编辑器展示和应用。`.yan` 原文仍是规范输入，LSP 不能执行 Rust 审查视图，也不能替 Parser、测试、fuel 或内容哈希做决定。

## 诊断如何产生

每次 open/change 都从编辑器快照重新运行正式 Reader/Parser。解析失败时发布稳定 `READ_*` / `PARSE_*` code、UTF-16 range 和人类消息；解析成功且是 language v4 时继续运行类型与效果分析。

通知本身没有 JSON-RPC response。若 change 版本倒退、携带增量 `range`、超过大小或形状不合法，server 不修改现有快照，只发送不含源码和 URI 的 `window/logMessage`。

## Hover 怎样避免猜名字

hover 不是按单词表搜索源码。bounded Reader 只定位光标下一个 symbol 及其精确 span；正式 AST 判断它是否真是可执行 special form/变量，词法符号索引先解析全局 `def`、参数、顺序 `let` 和 pattern binding，再考虑 core primitive、构造器、Schema 或 Library operation。因此局部参数叫 `log` 时不会显示日志 capability，`'(cond log)`、字符串和注释里的同名文本也没有代码提示。

提示使用 plaintext，最多 8 KiB，不包含 HTML、命令链接、源码值、URI 或宿主自由文本。special form 显示语法、最低 language version 与短路/作用域语义；core primitive 显示类型、effect/capability 和适用的 fuel 规则；`text@1` 一类 Library operation 的参数、返回类型和 fuel 公式直接读取可信 `LibraryContract`；用户函数显示推断/声明类型、传递 capability effect 与适用的 `expression-v1` 节点路径。返回 range 只覆盖被命中的 UTF-16 token，而不是整个 definition。

## Completion 怎样保持可执行

completion 不是把字典中的所有名字都塞给编辑器。bounded Reader 先判断光标属于顶层、表达式、类型还是 Schema；正式 Program 可用时，符号索引再按参数、顺序 `let`、match arm 和嵌套遮蔽计算当前位置可见 binding。局部同名 binding 优先于 primitive，注释、字符串和 quote data 不提供候选。

form 和 core primitive 复用 hover 的版本化目录。primitive 不高于当前 language version；`log`、`now-ms` 与 `kv-*` 还要求程序已声明对应 capability。Library 候选只来自程序声明的精确版本和 Rust 端可信 `LibraryContract`，不会猜一个未导入的 crate 或包。Reader 尚能理解结构、但完整 Program 因正在编辑而未通过 Parser 时，只提供可证明上下文正确的 form 候选。

返回的是标准 `CompletionList`，`isIncomplete:false`；每项只携带 label、plaintext 文档、稳定排序和当前 symbol 的精确 UTF-16 `TextEdit`。没有 snippet、command、自动 import、文件读取或跨文档 edit。单次最多 128 项、候选文本合计最多 256 KiB；超限返回 `LSP_COMPLETION_LIMIT`，不截断成一个看似完整的列表。

## 有界协议

- JSON-RPC body 最多 32 MiB；
- header 总计最多 16 KiB、32 行，单行最多 8 KiB；
- 单份源码最多 4 MiB；
- 最多 32 个打开文档，总源码最多 16 MiB；
- URI 最多 4 KiB；
- hover plaintext 最多 8 KiB，六倍 JSON 转义上界仍小于消息限制；
- completion 最多 128 项、候选文本合计最多 256 KiB，超限失败关闭；
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
- 没有 rename、semantic tokens、code action；
- 没有 Tree-sitter grammar、Neovim 安装包或 macOS/Arm Extension Host 平台验收；
- 没有跨 Bundle/package 的多文件链接导航。

实现入口：[符号索引](/source/rust/crates/yanshu-syntax/src/symbol.rs.txt)、[Completion 候选](/source/rust/crates/yanshu-lsp/src/completion/mod.rs.txt)、[Completion 上下文](/source/rust/crates/yanshu-lsp/src/completion/context.rs.txt)、[Hover 解析](/source/rust/crates/yanshu-lsp/src/hover/mod.rs.txt)、[Hover 目录](/source/rust/crates/yanshu-lsp/src/hover/catalog.rs.txt)、[协议 framing](/source/rust/crates/yanshu-lsp/src/protocol.rs.txt)、[文档与导航](/source/rust/crates/yanshu-lsp/src/document.rs.txt)、[server 生命周期](/source/rust/crates/yanshu-lsp/src/server.rs.txt)。完整契约见 [v0.12 规格](/source/docs/specs/v0.12.md.txt)。
