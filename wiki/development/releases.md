# 下载与验证 v0.12.0

v0.12.0 是当前公开发布版。你可以从 [GitHub Release v0.12.0](https://github.com/Yukk1o/yanshu/releases/tag/v0.12.0) 下载命令行工具或 VS Code 扩展，不需要先安装 Rust。

::: warning 当前支持的平台
官方二进制覆盖 Windows x86-64（MSVC）和 Linux x86-64（GNU）。目前没有 macOS、ARM、系统安装器、crates.io 包或 VS Code Marketplace 安装入口。
:::

## 选择下载内容

| 你要做什么 | 下载什么 |
| --- | --- |
| 使用命令行、MCP 或 LSP | 对应平台的 `yanshu-v0.12.0-<target>.zip` |
| 在 VS Code 中编辑 `.yan` | 对应平台的 `yanshu-vscode-0.12.0-<platform>.vsix` |
| 审计依赖 | `yanshu-v0.12.0-{cli,mcp,lsp,vscode}.cdx.json` 四份 SBOM |
| 验证整套发布 | Release 页面中的全部 12 个资产 |

每个平台 ZIP 都同时包含：

- `yanshu`：检查、格式化、运行与编译程序的 CLI；
- `yanshu-mcp`：给 Codex、Claude Code、OpenCode 使用的只读 MCP server；
- `yanshu-lsp`：编辑器语言服务。

VSIX 已经内置对应平台的 LSP，不需要再单独配置 server。安装方法见 [VS Code 使用指南](/development/vscode)，Agent 接入见 [MCP 使用指南](/development/mcp)。

## 下载并解压工具包

Windows PowerShell：

```powershell
gh release download v0.12.0 --repo Yukk1o/yanshu --pattern "yanshu-v0.12.0-x86_64-pc-windows-msvc.zip"
gh attestation verify .\yanshu-v0.12.0-x86_64-pc-windows-msvc.zip --repo Yukk1o/yanshu
Expand-Archive .\yanshu-v0.12.0-x86_64-pc-windows-msvc.zip -DestinationPath C:\Tools
```

Linux x86-64：

```bash
gh release download v0.12.0 --repo Yukk1o/yanshu \
  --pattern 'yanshu-v0.12.0-x86_64-unknown-linux-gnu.zip'
gh attestation verify ./yanshu-v0.12.0-x86_64-unknown-linux-gnu.zip \
  --repo Yukk1o/yanshu
unzip yanshu-v0.12.0-x86_64-unknown-linux-gnu.zip -d "$HOME/.local/opt"
```

解压后，把版本目录加入 `PATH`，或在编辑器和 Agent 配置中使用可执行文件的绝对路径。

## 三种验证分别证明什么

### 1. `SHA256SUMS`：检查下载内容是否损坏

下载资产和 `SHA256SUMS` 后，在 Linux、Git Bash 或其它带 `sha256sum` 的环境中运行：

```bash
sha256sum --check SHA256SUMS
```

它会重新计算文件哈希，适合发现下载损坏或文件被替换。但校验和文件和资产来自同一下载位置；如果二者一起被替换，仅检查 SHA-256 并不能证明发布者身份。

### 2. 发布验证器：检查 schema v3 的完整闭包

先把 Release 的全部 12 个资产放到同一目录，再从 Yanshu 仓库根目录运行：

```powershell
gh release download v0.12.0 --repo Yukk1o/yanshu --dir .runtime\release-v0.12.0
node scripts/verify-release.mjs .runtime\release-v0.12.0
```

验证器会检查 `SHA256SUMS`、`yanshu-v0.12.0.release.json`、两个平台构建记录、两个 ZIP、两个 VSIX 和四份 SBOM 是否互相引用且哈希一致。它证明“下载目录与 schema v3 发布清单内部一致”，仍不单独证明这些文件来自该 GitHub 仓库。

### 3. GitHub attestation：验证来源证明

对你真正要安装或分发的每个资产运行：

```powershell
gh attestation verify <资产路径> --repo Yukk1o/yanshu
```

这会验证 GitHub 记录的无密钥构建证明，并把资产绑定到 `Yukk1o/yanshu`。它不替你判断程序是否无 Bug，也不替代本机防病毒、组织软件准入和运行时权限控制。

## 推荐的安装检查

普通使用至少完成：

1. 从 v0.12.0 Release 下载与你平台匹配的资产；
2. 对要安装的 ZIP 或 VSIX 运行 `gh attestation verify`；
3. 不带参数运行 `yanshu`，确认得到结构化 `CLI_USAGE`；随后按 [快速开始](/guide/quickstart) 运行 `hello.yan`；
4. 需要归档、镜像或组织内再分发时，再下载全部资产并运行发布验证器。

Yanshu 仍处于早期阶段，且项目主要由 AI 生成，可能存在大量 Bug。不要因为来源证明通过，就跳过业务测试、最小权限和回滚准备。
