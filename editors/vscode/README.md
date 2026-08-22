# 衍术 Yanshu for Visual Studio Code

这是实验性的 `.yan` 语言扩展。它提供：

- `.yan` 文件识别、括号与行注释配置；
- 与当前语言表层语法一致的基础 TextMate 高亮，以及 AST/符号索引驱动的全文 semantic tokens；
- 通过独立 safe-Rust `yanshu-lsp` 提供诊断、关键字/函数/绑定的精确 hover、作用域感知补全、同文件全局/局部跳转、引用与防捕获重命名、全文格式化，以及 Rust 风格只读审查面板。

最低支持 VS Code 1.101；该版本开始使用 Node 22 extension host，与当前 language client 和 bundle 目标一致。

## 打开只读审查面板

打开 `.yan` 后，点击编辑器标题栏的预览图标，或从命令面板运行 **衍术 Yanshu: 打开 Rust 风格只读审查**。扩展会在旁侧显示当前编辑器快照的 `rust-readonly-v3` 语义投影；源码继续编辑时，面板在 250 ms debounce 后刷新。

面板不是 `.rs` 文件，也没有 VS Code `TextDocument` 编辑模型。它关闭脚本和本地资源，使用拒绝默认的 CSP，并在生成 HTML 前转义全部审查文本。它只能查看和复制，不能保存、执行或反向写回 `.yan`。单次预览输入最多 512 KiB、投影文本最多 4 MiB；Parser、类型或效果诊断未通过时显示不含源码的失败状态。

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

## Extension Host 端到端测试

先构建 release server，再启动固定的 VS Code 1.101.2 测试实例：

```powershell
cargo build --locked --release -p yanshu-lsp
Set-Location editors\vscode
npm run test:e2e
```

测试使用临时扩展副本和独立 user-data/extensions 目录，验证激活、`.yan` 识别、诊断、用户函数类型/节点 hover、关键字语法 hover、全局函数/局部参数补全及精确替换范围、关键词/类型/函数/参数 semantic tokens、同文件全局/局部跳转与引用、局部绑定重命名、格式化 edit，以及审查命令打开无可编辑文本模型的独立 Webview Panel。下载阶段可以使用宿主代理；测试实例启动前会移除代理和凭据形状的环境变量。下载缓存只存放在忽略的 `.vscode-test/`。若要使用当前机器上的独立 VS Code 安装，可用绝对路径设置 `YANSHU_VSCODE_EXECUTABLE`；该安装正在运行或更新时应使用默认下载副本。

CI 在 Windows x64 和 Linux x64（Xvfb）上运行同一套测试并生成对应 VSIX。测试 bundle 位于 `out/test/`，明确排除在 VSIX 之外。

## 安全边界

- `.yan` 仍是唯一规范源码；Rust 风格审查视图不是输入。
- 扩展只把编辑器中已打开的完整文档快照交给 LSP，不自行读取 URI 文件。
- server 子进程不会继承名称包含 key、token、secret、password、credential 或 auth 的环境变量。
- extension client 会 bundle 为单个 CommonJS 入口，VSIX 不携带开发依赖或散落的 `node_modules`。
- Extension Host 测试 bundle 不进入 VSIX；测试所用编辑器、用户数据和扩展目录也不进入产物。
- 打包器从锁定的生产依赖闭包生成第三方许可证正文；缺失、symlink 或超限许可证会失败关闭。
- formatting 只返回 `TextEdit[]`；是否应用由 VS Code 和用户决定。
- rename 只返回绑定当前文档版本的同文件 `WorkspaceEdit`；捕获、解析变化或符号图变化会被 server 拒绝，是否应用仍由 VS Code 和用户决定。
- review panel 只消费打开快照；不读取 URI 文件、不创建 `.rs`、不暴露编辑模型或执行入口。
- 当前没有 Tree-sitter 或 semantic token range/delta；completion、references 与 rename 仅限当前打开文档。

项目主页：[Yanshu Wiki](https://yukk1o.github.io/yanshu/) · [GitHub](https://github.com/Yukk1o/yanshu)
