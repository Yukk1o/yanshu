# Formatter 与稳定节点 ID

v0.12 的第一项工具能力不是新的语法，而是让人类、CI 和 AI 对同一份 `.yan` 得到稳定布局。formatter v1 已实现；它不执行程序，也不会自动覆盖源码。

## 只读格式化

查看候选结果：

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  format examples\expenses\service.yan
```

命令返回 JSON：

```json
{
  "ok": true,
  "changed": true,
  "formatterVersion": 1,
  "formattedSource": "(program\n  ...\n)\n"
}
```

`formattedSource` 是经过重新解析和语义比对的候选源码，但命令不会写文件。这样 Codex、Claude Code、OpenCode 或编辑器可以先展示 diff，再由用户或受约束编辑动作决定是否应用。

CI 只需要检查：

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  format examples\expenses\service.yan --check
```

已经规范化时返回 `ok: true`；需要格式化时返回稳定错误码 `FORMAT_REQUIRED` 和非零退出码，不把整份源码塞进错误报告。

## formatter 会证明什么

每次成功结果都经过：

1. Reader/Parser 校验原源码和 language version；
2. 保留注释的 concrete tree 排版；
3. Reader/Parser 重新解析输出；
4. 比较无 span 的 Program inspection；
5. 比较注释文本和顺序；
6. 第二次格式化逐字节一致。

因此 formatter bug 会失败关闭。它不会因为“看起来差不多”就改写 `.yan`。字符串与 atom lexeme 保留；`()`、`[]`、`{}` 这三种 Reader 列表分隔符会规范化为 `()`；注释行尾的空格可被删除。

默认输入上限沿用 Reader 的 4 MiB、10,000 节点与 128 层嵌套；输出在每次追加前受同一个 4 MiB source 上限约束。默认行宽是 100、缩进是 2。

## 稳定表达式节点路径

Parser 现在还能为 expression 生成类似下面的工具 ID：

```text
expression-v1/definition/decide-expense/function/body/let/body/if/condition
```

ID 使用 definition 名和 AST 角色，不使用行号、列号或 byte offset。所以只改空白、移动注释或运行 formatter 后，节点 ID 不变，source span 则更新到新位置。

它不是内容哈希：重命名 definition、插入分支或调整参数顺序时，对应 ID 可以变化。后续 LSP 与结构化 diff 会在这个基础上处理跨编辑匹配，而不是声称任意修改都能保留永久身份。

## 当前没有什么

- 没有原地覆盖或 `--write`；
- 没有从 Rust 风格审查视图反向生成 `.yan`；
- 没有 Tree-sitter、LSP、MCP 或编辑器插件；
- 没有多文件 Bundle/package 批量格式化。

完整工程契约见 [v0.12 规格](/source/docs/specs/v0.12.md.txt)。公共 API 在 [yanshu-format](/source/rust/crates/yanshu-format/src/lib.rs.txt)，注释树与排版分别在 [cst.rs](/source/rust/crates/yanshu-format/src/cst.rs.txt) 和 [render.rs](/source/rust/crates/yanshu-format/src/render.rs.txt)，节点定位在 [node_id.rs](/source/rust/crates/yanshu-syntax/src/node_id.rs.txt)。
