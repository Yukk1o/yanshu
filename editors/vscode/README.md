# 衍术 Yanshu for Visual Studio Code

这是实验性的 `.yan` 语言扩展。它提供：

- `.yan` 文件识别、括号与行注释配置；
- 与当前语言表层语法一致的基础 TextMate 高亮；
- 通过独立 safe-Rust `yanshu-lsp` 提供诊断、hover、同文件全局跳转和全文格式化。

最低支持 VS Code 1.101；该版本开始使用 Node 22 extension host，与当前 language client 和 bundle 目标一致。

> **不要用于生产或敏感数据。** 衍术及本扩展主要由 AI 编程代理协助生成，尚未经过充分独立人工审计，可能包含大量缺陷。

## 安装和启动

平台专用 VSIX 会携带同平台的 `yanshu-lsp`。如果扩展中没有对应 binary，它会查找宿主 `PATH` 中的 `yanshu-lsp`。也可以在 VS Code 用户设置中指定可信的绝对路径：

```json
{
  "yanshu.server.path": "E:\\tools\\yanshu-lsp.exe"
}
```

该设置是 machine scope，工作区不能用仓库内配置替换 server。扩展不运行 `cargo`，不执行工作区脚本，也不会把相对路径解析到当前项目。

## 从源码生成本机 VSIX

在仓库根目录构建 release server：

```powershell
cargo build --locked --release -p yanshu-lsp
Set-Location editors\vscode
npm ci
npm run package
```

产物位于 `editors/vscode/dist/`，只包含当前宿主平台的 server。`YANSHU_LSP_BINARY` 可以覆盖打包输入，但必须是绝对路径、普通文件且不超过 128 MiB。

安装示例：

```powershell
code --install-extension dist\yanshu-vscode-0.10.0-win32-x64.vsix
```

## 安全边界

- `.yan` 仍是唯一规范源码；Rust 风格审查视图不是输入。
- 扩展只把编辑器中已打开的完整文档快照交给 LSP，不自行读取 URI 文件。
- server 子进程不会继承名称包含 key、token、secret、password、credential 或 auth 的环境变量。
- extension client 会 bundle 为单个 CommonJS 入口，VSIX 不携带开发依赖或散落的 `node_modules`。
- 打包器从锁定的生产依赖闭包生成第三方许可证正文；缺失、symlink 或超限许可证会失败关闭。
- formatting 只返回 `TextEdit[]`；是否应用由 VS Code 和用户决定。
- 当前没有 completion、rename、references、Tree-sitter 或局部 binding 跳转。

项目主页：[Yanshu Wiki](https://yukk1o.github.io/yanshu/) · [GitHub](https://github.com/Yukk1o/yanshu)
