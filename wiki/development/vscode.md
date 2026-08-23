# 在 VS Code 中编写衍术

官方 VS Code 扩展为 `.yan` 提供语法高亮、诊断、hover、补全、定义/引用、重命名、格式化和 Rust 风格只读审查。当前最低支持 VS Code 1.101。

::: warning 实验性软件
扩展与语言主要由 AI 编程代理协助生成，可能存在大量 Bug。请先在非生产项目评估。
:::

## 安装

从 [Yanshu v0.12.0 Release](https://github.com/Yukk1o/yanshu/releases/tag/v0.12.0) 下载平台对应的 VSIX：

- Windows x64：`yanshu-vscode-0.12.0-win32-x64.vsix`
- Linux x64：`yanshu-vscode-0.12.0-linux-x64.vsix`

终端安装：

```powershell
code --install-extension .\yanshu-vscode-0.12.0-win32-x64.vsix
```

也可以在 VS Code 的“扩展”视图打开 `…` 菜单，选择 **从 VSIX 安装**。安装完成后重新加载窗口，再打开 `.yan` 文件。

当前尚未发布到 Marketplace，也没有 macOS 或 Arm 平台包。

## 你会获得什么

| 功能 | 行为 |
| --- | --- |
| 诊断 | 正式 Parser 错误；v4 还包含类型和 effect/capability 错误 |
| Hover | 查看 form 语法、primitive/Library 契约、用户函数类型和可达 capability |
| 补全 | 只建议当前版本、作用域、capability 和 Library 声明中可用的名字 |
| 导航 | 同文档的全局 `def`、参数、顺序 `let` 和模式绑定定义/引用 |
| 重命名 | 返回作用域感知的编辑；发生名字捕获时拒绝 |
| 格式化 | 在编辑器确认后应用 formatter v1 的全文档 edit |
| 审查视图 | 以 Rust 心智模型显示类型、模式匹配与 `log!` 等副作用标记 |

定义、引用和重命名当前是同文档能力，还不支持跨 Bundle/package 的多文件导航。

## 打开 Rust 风格只读审查

打开 `.yan` 后，点击编辑器标题栏的预览图标，或从命令面板运行：

```text
衍术 Yanshu: 打开 Rust 风格只读审查
```

面板会跟随当前编辑器快照更新。它可以选择和复制，但：

- 不是 Rust 源码；
- 不能执行；
- 不能编辑或保存回 `.yan`；
- `.yan`、Bundle manifest 和测试仍是规范输入。

`name!(...)` 表示该调用直接或间接使用 capability。审查头还会明示 `Int` 为任意精度整数，以及只有 `Bool(false)` 为假的条件语义。

## 格式化文件

使用 VS Code 的 **格式化文档** 命令即可。LSP 只返回 edit，VS Code 在用户操作下应用；server 不会直接修改磁盘文件。

如果只想检查 CI 格式：

```powershell
.\yanshu.exe format policy.yan --check
```

## 排查扩展未启动

1. 确认右下角语言模式是 **Yanshu**，文件后缀是 `.yan`。
2. 确认 VSIX 平台与当前 VS Code 相同，Windows 包不能用在 Linux。
3. 从 **输出 → Yanshu Language Server** 查看不含源码的启动错误。
4. 若使用自定义 server，`yanshu.server.path` 必须是 machine setting 中的绝对普通文件路径。

```json
{
  "yanshu.server.path": "E:\\tools\\yanshu-lsp.exe"
}
```

扩展优先使用该设置，其次使用 VSIX 内置且平台匹配的 server，最后才查找 `PATH`。工作区不能通过项目级 setting 替换你的 server。

其他编辑器作者可继续阅读 [LSP 接入](/development/lsp)。
