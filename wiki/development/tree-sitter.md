# Tree-sitter 展示语法

`editors/tree-sitter-yanshu` 为 `.yan` 提供增量 concrete syntax tree（CST）和标准查询，供 Neovim、Zed、代码浏览器等 Tree-sitter 消费者建立折叠、标签与基础高亮。当前仓库还没有发布独立 npm 包或这些编辑器的安装包。

## 它覆盖什么

grammar 覆盖当前 v1-v4 源码的可见结构：

- Reader 支持的 `()`、`[]`、`{}` 三组等价列表分隔符；
- 行注释、字符串与合法转义、整数、四种布尔拼写和 quote datum；
- program 声明、Library、imports、Schema、data、signature、route 与 export；
- type expression、条件、函数、顺序 `let`、调用、`match` 与嵌套 pattern；
- `highlights.scm`、`locals.scm`、`folds.scm`、`tags.scm` 标准查询。

本地验证：

```powershell
Set-Location editors\tree-sitter-yanshu
npm ci
npm run check
```

`npm run check` 会验证生成制品没有漂移、corpus 预期树不变、四组 query 可编译，并让 Tree-sitter 与 formatter 的正式 Reader/Parser 往返路径接受同一组有界仓库源码。

## 为什么它不是另一个 Parser

| 层 | 能决定什么 | 不能决定什么 |
| --- | --- | --- |
| Tree-sitter | 编辑中的括号结构、节点范围、基础高亮、折叠和标签 | 版本合法性、名称解析、类型/effect、capability、fuel、hash |
| `yanshu-syntax` Reader/Parser | 规范 AST、版本门禁、稳定诊断、Schema/type/form 合法性 | 宿主是否授权晋升 |
| 分析、运行时与宿主 | 类型/effect、能力闭包、受限执行、测试、密封和晋升 | 把容错 CST 冒充规范源码 |

Tree-sitter 会为了编辑体验产生 `ERROR` 或 `MISSING` 节点，也可能暂时容纳正式 Parser 最终拒绝的半成品。任何消费者都不得把这棵树送入执行、内容哈希、Bundle 密封、capability 分析或源码回写。`.yan` 与密封 manifest 始终是规范输入。

## 名称解析边界

`locals.scm` 只描述函数参数和 match arm 这类 Tree-sitter 能可靠表达的词法范围。它刻意不近似顺序 `let`：每个 binding 只对后续 binding 和 body 可见，通用 locals query 不能无损表达这条语义。

精确的遮蔽、definition/references、防捕获 rename 和 semantic tokens 继续由 `yanshu-lsp` 的正式 AST 与 `SymbolIndex` 负责。Tree-sitter 基础高亮与 LSP 语义高亮可以共存，但发生分歧时只能信正式 Parser/LSP。

## Safe Rust 边界

锁定的 Tree-sitter CLI 会生成 `parser.c`、grammar/node type JSON 和所需 C headers。这些文件只留在编辑器 grammar 包中。第一方 Rust crate 不链接 Tree-sitter，也没有需要 FFI `unsafe` 的 Rust binding，因此不会削弱仓库的 safe-Rust 规则。

生成的原生 parser 本身不是沙箱，也没有 Yanshu fuel 或内存计量。具体编辑器适配必须在调用前限制文档大小；为了与正式入口一致，应采用 Reader 的 4 MiB source ceiling。

下一步是为具体编辑器制作最小安装适配，并继续把 Tree-sitter 限定在显示层；不是把 LSP 或正式 Parser 替换掉。
