# 可验证发布

Yanshu 的 GitHub Release 不是“编译完随手上传”。v0.11 把版本身份、构建一致性、依赖清单、内容校验和来源证明连成一条机器可验证的证据链；v0.12 又把 LSP 和平台 VSIX 纳入同一个闭包。

::: warning 当前范围
当前公开里程碑仍是 v0.10；已有发布不会被补写或替换。新的合格标签会覆盖 Windows x86-64 与 Linux x86-64 的 `yanshu` CLI、只读 `yanshu-mcp`、`yanshu-lsp` 和对应平台 VSIX；这不代表 crates.io、VS Code Marketplace、macOS、ARM、安装器或生产稳定性。
:::

## 标签怎样获得发布资格

只有 push 到仓库的稳定标签会进入 publish job，而且必须同时满足：

1. 是注解式 Git tag，不是 lightweight tag；
2. 指向 `origin/main` 已包含的 commit；
3. 名称精确等于 `v` + workspace 版本，例如 `v0.11.0`；
4. 第一方 crate 全部继承相同版本、MSRV、许可证和 `publish = false`；
5. 第一方 path dependency 的精确版本没有漂移。

Pull request 与手动触发只能做发布演练，没有 `contents`、OIDC 或 attestation 写权限。

## 每个发布里有什么

```text
annotated tag on main
        │
        ├─ Linux：全新 target A ─┐
        │   CLI + MCP + LSP       ├─ 三个程序分别字节一致 ─ deterministic ZIP
        │       全新 target B ───┘                   └─ linux-x64 VSIX × 2
        │
        ├─ Windows：全新 target A ─┐
        │     CLI + MCP + LSP       ├─ 三个程序分别字节一致 ─ deterministic ZIP
        │         全新 target B ───┘                   └─ win32-x64 VSIX × 2
        │
        ├─ CLI / MCP / LSP / VS Code CycloneDX 1.5 SBOM
        └─ build records + release manifest + SHA256SUMS
                                      │
                                      ▼
                         GitHub OIDC keyless provenance
```

每个平台有一个 ZIP、一个平台 VSIX 和一个 `.build.json`；ZIP 同时装入 `yanshu`、`yanshu-mcp` 与 `yanshu-lsp`。构建记录写明 target、源码 commit、源码时间、Rust/Cargo/Node/vsce 实际版本，以及三个二进制、VSIX 和归档各自的大小、smoke 契约与 SHA-256。总 release manifest 再把两个平台、四份 SBOM、VSIX 和构建记录闭合，`SHA256SUMS` 覆盖所有 payload 与 manifest。

ZIP 不使用会随实现漂移的压缩器：条目排序、固定时间和 mode、只接受普通相对路径。Windows 使用 MSVC `/Brepro` 消除 PE timestamp 与 CodeView 随机构建标识。VSIX 使用精确锁定的 vsce 和源码时间构建两次，并分别携带两次独立构建的 LSP；任一字节不同都拒绝发布。Rust 与 npm SBOM 的随机 serial 和墙钟时间会被规范化，checkout 本地 `file:`/绝对路径会绑定到仓库 commit 身份；仍残留 runner 路径就拒绝发布。

## 校验和不是签名

攻击者若能同时替换 ZIP 与 `SHA256SUMS`，内容校验仍会“通过”。因此下载后必须做两层验证：

```powershell
node scripts/verify-release.mjs <下载目录>
gh attestation verify <下载的资产> --repo Yukk1o/yanshu
```

第一条验证目录内 checksum 与 release manifest 的完整闭包；第二条验证 GitHub 为 `Yukk1o/yanshu` 的工作流记录过该资产的 keyless provenance。仓库不保存长期签名私钥。

## “可复现”当前承诺到哪里

同一平台 job 在两个全新的 Cargo target 目录完整构建 CLI、MCP 和 LSP，只有三个程序各自逐字节相同才继续。CLI 必须返回稳定 usage JSON；MCP 必须真实完成 initialize 和 `tools/list`；LSP 必须完成有界的 `initialize`、`shutdown`、`exit` 握手，并声明 UTF-16 与只读审查契约。两个 VSIX 也必须逐字节相同。构建固定 Cargo/npm lock、Rust patch 工具链、Node 22、精确 vsce、`SOURCE_DATE_EPOCH`、源码路径映射与 release profile。

但 GitHub 托管 runner 与系统 linker 还不是按镜像 digest 固定的 hermetic 环境。因此当前证据是“同源码、同 runner 双构建一致”，不是“任何机器已必然重建同一 hash”。`.build.json` 把实际工具链写出来，后续独立 rebuilder 才能透明比较，而不是把差异藏起来。

## 本地检查

不会发布任何内容的快速检查：

```powershell
node --test scripts/release.test.mjs
node scripts/release-metadata.mjs
```

完整威胁模型与 Windows 本地双构建命令见[发布供应链契约](/source/docs/engineering/release-supply-chain.md.txt)，实际自动化见[release workflow](/source/.github/workflows/release.yml.txt)。
