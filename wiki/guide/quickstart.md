# 安装与 5 分钟上手

这一页只做三件事：安装 CLI，写一个函数，运行它。不需要先了解仓库结构或完整语言实现。

## 1. 安装 CLI

从 [Yanshu v0.12.0 Release](https://github.com/Yukk1o/yanshu/releases/tag/v0.12.0) 下载系统对应的压缩包：

- Windows x64：`yanshu-v0.12.0-x86_64-pc-windows-msvc.zip`
- Linux x64：`yanshu-v0.12.0-x86_64-unknown-linux-gnu.zip`

解压后的 `yanshu.exe` / `yanshu` 是 CLI，同目录还有编辑器和 Agent 可使用的 `yanshu-lsp` 与 `yanshu-mcp`。

Windows PowerShell：

```powershell
Set-Location C:\path\to\yanshu-v0.12.0-x86_64-pc-windows-msvc
.\yanshu.exe
```

Linux：

```bash
cd /path/to/yanshu-v0.12.0-x86_64-unknown-linux-gnu
./yanshu
```

没有参数时，CLI 会返回 `CLI_USAGE` 和非零退出码；这表示可执行文件已能正常启动。

::: tip 从源码运行
如果你已经 clone 仓库并安装 Rust 1.97，可以把下面命令中的 `.\yanshu.exe` 替换为 `cargo run --quiet --locked -p yanshu-cli --`。
:::

## 2. 创建 `hello.yan`

```lisp
(program
  (name hello)
  (version 4)
  (capabilities)

  (signature greet (fn (string) string))
  (def greet
    (fn (name)
      (string-append "你好，" name)))

  (export greet))
```

这个程序声明了一个不需要副作用的导出函数 `greet`。它接收字符串，返回字符串。

先让 Parser 和 v4 类型/效果分析器检查文件：

```powershell
.\yanshu.exe check hello.yan
```

成功时返回 JSON，顶层包含 `"ok": true`，并列出 `greet` 的类型与空 capability 集合。

## 3. 准备参数

导出函数的参数使用 JSON 数组传给 CLI。创建 `arguments.json`：

```json
["世界"]
```

## 4. 编译并运行

```powershell
.\yanshu.exe compile-bytecode hello.yan hello.ybc.json
.\yanshu.exe run-bytecode hello.yan hello.ybc.json greet arguments.json
```

第二条命令的结果中包含：

```json
{
  "ok": true,
  "result": "你好，世界"
}
```

实际输出还会包含内容哈希、`logEvents` 和 fuel 消耗报告。字节码在运行时会与 `hello.yan` 重新校验；修改源码后应重新编译。

## 5. 安装 VS Code 扩展

从同一 Release 下载与平台匹配的 VSIX：

- Windows x64：`yanshu-vscode-0.12.0-win32-x64.vsix`
- Linux x64：`yanshu-vscode-0.12.0-linux-x64.vsix`

```powershell
code --install-extension .\yanshu-vscode-0.12.0-win32-x64.vsix
```

重新打开 `hello.yan`后，你应该能看到语法高亮、诊断、hover、补全、定义/引用、重命名和格式化。编辑器标题栏的预览命令会打开 Rust 风格只读审查视图；它不是 Rust 源码，也不能保存回 `.yan`。

## 常见问题

### `PROGRAM_FEATURE_REQUIRES_VERSION`

源码使用了当前 `(version ...)` 不支持的 form。新程序请使用 `(version 4)`；升级旧程序时也要补齐 v4 的导出 `signature`。

### `TYPE_*` 或 `EFFECT_*`

v4 会在运行前检查参数、返回值和 capability 闭包。先看 JSON 中的稳定 `code` 和 source span，不要只搜索终端文案。

### Windows 报“无法运行此应用”

确认下载的是 `x86_64-pc-windows-msvc` 压缩包，不是 Linux 产物。当前正式 Release 尚未提供 macOS 或 Arm 构建。

下一步阅读[语法入门](/language/syntax)，或直接跟随[费用审批实战](/guide/expense-app)。
