# VS Code 扩展

`editors/vscode` 是 `.yan` 的首个编辑器安装包。它按 VS Code 官方 language client 架构启动独立 `yanshu-lsp`，提供文件识别、基础 TextMate 高亮、诊断、关键字/函数/绑定的精确 hover、同文件全局/局部跳转与引用、全文格式化，以及旁侧 Rust 风格只读审查面板。

最低支持 VS Code 1.101；官方从该版本把 Node extension host 升级到 Node 22，与当前 client 和 bundle target 一致。

::: warning 实验性软件
扩展与语言主要由 AI 编程代理协助生成，尚未经过充分独立人工审计。不要用于生产、关键业务或敏感数据。
:::

## 像 Markdown Preview 一样审查

打开 `.yan` 后，点击编辑器标题栏的预览图标，或从命令面板运行 **衍术 Yanshu: 打开 Rust 风格只读审查**。旁侧面板展示当前编辑器版本对应的 `rust-readonly-v3`；继续编辑源码时，它在 250 ms debounce 后重新请求当前快照。

这不是伪装成只读的虚拟 `.rs` 文件。面板使用独立 `yanshu.review` Webview，没有 `TextDocument`、保存或格式化入口；脚本关闭、本地资源根为空、CSP 默认拒绝，源码标签和投影文本在进入 HTML 前完整转义。面板可以选择和复制文本，但不能编辑、执行或反向写回 `.yan`。

预览输入最多 512 KiB，输出最多 4 MiB，同时最多 32 个面板。版本漂移、Parser/类型/效果失败或超限都失败关闭；错误页不回显源码、URI 或 server 自由文本。

## 从源码生成本机 VSIX

```powershell
cargo build --locked --release -p yanshu-lsp
Set-Location editors\vscode
npm ci
npm run package
```

产物位于 `editors/vscode/dist/`，文件名包含版本和平台，例如 `yanshu-vscode-0.10.0-win32-x64.vsix`。安装：

```powershell
code --install-extension dist\yanshu-vscode-0.10.0-win32-x64.vsix
```

这个本地打包流程只生成当前宿主平台的 VSIX。CI 会在 Windows x64 与 Linux x64 分别测试并暂存平台包，但这不等于已经完成 Marketplace、macOS 或 Arm 发布。

## Extension Host 自动验收

在仓库根目录先构建 release server，再进入扩展目录运行：

```powershell
cargo build --locked --release -p yanshu-lsp
Set-Location editors\vscode
npm run test:e2e
```

测试固定 VS Code 1.101.2，在系统临时目录组装最小扩展副本、内置 LSP 和隔离 user-data/extensions 目录，自动验证：

- 扩展激活与 `.yan` language ID；
- Parser 诊断；
- 用户函数 hover 的类型与稳定 expression node；
- `fn` 等关键字 hover 的语法和版本说明；
- 同文档全局与局部 parameter definition；
- 同文档全局与局部 parameter references；
- formatter 只返回 edit、不直接修改文档。
- 审查命令打开独立 Webview，且没有 `yanshu-review` 可编辑文档。

下载测试编辑器时可以沿用宿主代理；Extension Host 启动前会移除代理、过滤凭据形状的环境变量，并关闭更新与遥测。测试编辑器下载缓存位于忽略的 `.vscode-test/`；测试 bundle 明确排除在 VSIX 之外。CI 在 Windows 与 Linux/Xvfb 上执行同一验收。

## Server 怎样选择

启动顺序是：

1. 用户 machine setting `yanshu.server.path` 指向的绝对普通文件；
2. VSIX 内与当前平台和架构完全匹配的 binary；
3. 宿主 `PATH` 中的 `yanshu-lsp` / `yanshu-lsp.exe`。

手工配置示例：

```json
{
  "yanshu.server.path": "E:\\tools\\yanshu-lsp.exe"
}
```

该设置不是 workspace scope，因此仓库不能通过 `.vscode/settings.json` 把 server 换成项目内可执行文件。相对路径和不存在的配置都会拒绝；扩展也不会自动运行 `cargo` 或工作区脚本。

## 扩展边界

- `.yan` 是规范源码，TextMate token 颜色不参与语言语义；
- extension client 只同步编辑器已打开的快照；Rust server 不按 URI 读取磁盘；
- server 子进程不会继承名称含 key/token/secret/password/credential/auth 的环境变量；
- extension client bundle 为单个 CommonJS 入口，VSIX 不携带开发依赖或散落的 `node_modules`；
- 打包器从生产依赖闭包生成排序、有界的第三方许可证正文，缺失或超限时拒绝；
- formatting 仅返回 `TextEdit[]`，由 VS Code 和用户决定是否应用；
- review 只消费打开快照并生成无脚本展示面板，不创建 `.rs` 或可编辑虚拟文档；
- 平台包只接受不超过 128 MiB 的非 symlink release binary，并记录 SHA-256 和字节数；
- npm 依赖精确固定，官方 registry 审计当前为 0 known vulnerabilities。
- Extension Host 使用隔离配置目录；测试进程不继承凭据形状的环境变量。

## 仍未实现

- Tree-sitter 与 semantic tokens；
- completion 和防捕获 rename；
- 跨 Bundle/package 的多文件 references；
- 自动生成 Windows/Linux/macOS 与 x64/Arm 全矩阵 VSIX 的发布工作流；
- Neovim、Zed、JetBrains 等安装包。

源码入口：[扩展 client](/source/editors/vscode/src/extension.ts.txt)、[审查面板控制器](/source/editors/vscode/src/review-preview.ts.txt)、[无脚本审查 HTML](/source/editors/vscode/src/review-html.ts.txt)、[server 选择与脱敏](/source/editors/vscode/src/server-command.ts.txt)、[语言 grammar](/source/editors/vscode/syntaxes/yanshu.tmLanguage.json.txt)、[VSIX 打包边界](/source/editors/vscode/scripts/package.cjs.txt)。LSP 能力见[最小 LSP Server](/development/lsp)。
