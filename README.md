<div align="center">

# 衍术 · Yanshu

### 程序即数据，演化皆可溯。

*Programs are data. Evolution leaves a trace.*

![status](https://img.shields.io/badge/status-experimental-f59e0b?style=flat-square)
![release](https://img.shields.io/badge/release-v0.12-6d5dfc?style=flat-square)
![language](https://img.shields.io/badge/language-v4-22c55e?style=flat-square)
![implementation](https://img.shields.io/badge/implementation-safe_Rust-b7410e?style=flat-square&logo=rust)
![license](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-2563eb?style=flat-square)
[![CI](https://github.com/Yukk1o/yanshu/actions/workflows/ci.yml/badge.svg)](https://github.com/Yukk1o/yanshu/actions/workflows/ci.yml)
[![Fuzz](https://github.com/Yukk1o/yanshu/actions/workflows/fuzz.yml/badge.svg)](https://github.com/Yukk1o/yanshu/actions/workflows/fuzz.yml)
[![Release](https://github.com/Yukk1o/yanshu/actions/workflows/release.yml/badge.svg)](https://github.com/Yukk1o/yanshu/actions/workflows/release.yml)

一门面向人类与 AI 协作的实验性、受限通用语言：候选代码可以持续生成，执行权与晋升权始终留在可审计的宿主边界内。

[快速体验](#快速体验) · [语言能力](#现在能做什么) · [安全边界](#安全边界) · [在线 Wiki](https://yukk1o.github.io/yanshu/) · [Wiki 源码](wiki/README.md) · [路线图](#项目状态)

</div>

> [!CAUTION]
> **实验性 AI 辅助软件，请勿用于生产。** 本项目由人类提出目标与安全边界，并在 AI 编程代理的大量协助下设计、实现和测试。代码尚未经过充分的独立人工审计，可能包含大量缺陷、安全问题、语义不一致及数据丢失风险。请勿用于生产环境、关键业务或敏感数据处理。
>
> **Experimental, AI-assisted software.** This project was designed, implemented, and tested with substantial assistance from AI coding agents under human-directed goals and security boundaries. It has not received sufficient independent human audit and may contain numerous bugs, security flaws, semantic inconsistencies, or data-loss risks. Do not use it in production, critical systems, or with sensitive data.

## 为什么是衍术

传统语言把源代码视为写给编译器的文本；衍术进一步把程序视为可以解析、哈希、比较、测试、审查和派生的数据。

AI 可以提出一个完整候选版本，但不能直接改写正在服务用户的活动程序。每个候选都必须经过同一条宿主持有的流水线：

```mermaid
flowchart LR
    A[".yan 规范源码"] --> B["Parser + 类型/效果分析"]
    B --> C["fuel 受限执行 + 完整测试"]
    C --> D["内容哈希 + 密封制品"]
    D --> E["人类只读审查"]
    E --> F["显式晋升 / 廉价回滚"]
```

这里的“运行期演化”不是让 LLM 绕过测试在线修改闭包，而是让运行系统持续产生隔离候选，再由确定的语言规则和发布门禁决定它能否成为后继版本。

## 一眼看懂

下面是一个 v4 模块。它声明了数据类型、函数签名和唯一允许的宿主能力 `log`：

```text
(program
  (name expense-policy)
  (version 4)
  (capabilities log)

  (data decision
    (approved (amount integer))
    (review (amount integer) (reason string))
    (rejected (reason string)))
  (export-types decision)

  (signature decide (fn (integer) decision))
  (def decide
    (fn (amount)
      (cond
        ((< amount 0)
          (rejected "negative amount"))
        ((>= amount 1000)
          (review amount "manual approval required"))
        (else
          (do
            (log amount)
            (approved amount))))))

  (export decide approved review rejected))
```

供人类审核时，同一程序会生成 Rust 风格的**只读语义视图**。`log!` 明确表示副作用；视图不能作为 Rust 或衍术源码重新执行：

```rs
// Generated semantic review — READ ONLY.
// semantic Int = bounded arbitrary-precision integer.

fn decide(amount: Int) -> Decision ![log] {
    if amount < 0 {
        Decision::Rejected { reason: "negative amount" }
    } else {
        log!(amount);
        Decision::Approved { amount }
    }
}
```

## 快速体验

需要仓库声明的 Rust 工具链；当前最低 Rust 版本是 1.97。在 PowerShell 中：

```ps1
# 解析并检查一个最小程序
cargo run --locked -p yanshu-cli -- `
  inspect examples\discount\v2.yan

# 执行 v4 语言契约
cargo run --locked -p yanshu-cli -- `
  conformance conformance\v4\manifest.json

# 生成人类可读、不可执行的 Bundle 审查视图
cargo run --locked -p yanshu-cli -- `
  review-bundle examples\bundles\typed-expense --text
```

构建 CLI 后，可以直接使用 `target\debug\yanshu.exe`：

```ps1
cargo build --locked -p yanshu-cli
.\target\debug\yanshu.exe inspect examples\discount\v2.yan
```

VS Code 扩展源码位于 `editors/vscode`。它识别 `.yan`、提供基础高亮，并通过独立 `yanshu-lsp` 显示诊断、关键字/函数/绑定的精确 hover、作用域感知补全、全局/局部跳转、同文档引用、防捕获重命名、格式化和 Rust 风格只读审查面板；固定 VS Code 1.101.2 的隔离 Extension Host 测试会在 Windows/Linux CI 验证这些能力。生成携带当前平台 server 的本机 VSIX：

```ps1
cargo build --locked --release -p yanshu-lsp
Set-Location editors\vscode
npm ci
npm run package
```

包含 v0.12 发布闭环的新稳定标签还会在 GitHub Release 提供 `win32-x64` 与 `linux-x64` VSIX；历史 Release 不会被补写。下载完整资产集并验证 manifest、checksum 与 GitHub provenance 后再安装，具体见[扩展说明](editors/vscode/README.md)。

`editors/tree-sitter-yanshu` 还提供面向 Neovim、Zed 等生态的增量语法树和标准 highlight/locals/folds/tags 查询。它是容错的只读显示层，不验证语言版本、类型、效果或 capability，也不参与执行和内容哈希：

```ps1
Set-Location editors\tree-sitter-yanshu
npm ci
npm run check
```

Codex、Claude Code 与 OpenCode 还可以通过独立的只读 MCP server 调用正式 inspect、formatter 和 Rust 风格审查链路。server 只接收源码文本，不读写工作区、不执行 guest，也不访问网络：

```ps1
cargo build --locked --release -p yanshu-mcp
codex mcp add yanshu -- E:\learn\yanshu\target\release\yanshu-mcp.exe
```

三种 Agent 的完整配置见 [MCP 使用页](wiki/development/mcp.md)。

运行真实任务服务及 11 个有状态场景：

```ps1
cargo run --locked -p yanshu-cli -- `
  test-service examples\tasks\service.yan examples\tasks\scenarios.json

.\scripts\serve-tasks.ps1
```

服务默认只监听 `127.0.0.1:8081`。这是本地验证入口，不是生产部署方案。

## 现在能做什么

|能力 |当前实现 |
|---|---|
|程序即数据 |S-expression AST、稳定 span、机器可读诊断与规范 JSON |
|业务表达 |短路 `and/or`、`cond`、集合处理、Schema、Result、模式匹配 |
|模块化 |用户数据类型、导出签名、密封 Bundle、模块链接 |
|静态审查 |类型推断、效果分析、capability 闭包、Rust 风格只读视图 |
|供应链 |内容寻址 package、闭包锁文件、全量重验 |
|受限执行 |调用深度、值边界、Reader 边界与显式 fuel 计量 |
|编译路径 |规范字节码、verifier、解释器/VM 差分与 WASM handle ABI |
|宿主生态 |安全 Rust Library Backend；guest 不能直接调用 crates.io 或 FFI |
|AI 开发 |DeepSeek/OpenAI-compatible HTTP、Codex/Claude Code/OpenCode CLI 后端，以及只读 MCP 语言工具 |
|编辑器工具 |有界 LSP、VS Code 平台包、Tree-sitter grammar 与标准查询 |
|生命周期 |候选注册、测试门禁、显式晋升、影子执行、审计事件与哈希回滚 |

编译一个锁定的费用审批 package：

```ps1
cargo run --locked -p yanshu-cli -- package-compile `
  .runtime\package-store `
  examples\packages\typed-expense\yanshu.lock.json `
  .runtime\typed-expense.ybc.json `
  .runtime\typed-expense.wasm
```

## 安全边界

衍术当前追求的是**结构性限制和可审计性**，不是已经获得证明的绝对安全。

- guest 没有 `eval`、隐式宿主访问、文件、网络、线程、动态库或任意 FFI。
- capability 必须显式声明、静态分析、由宿主注入，并在运行时再次核对。
- 读取、解析、Schema、值转换、标准库和 capability 返回都必须有边界并计费。
- 第一方 Rust 使用 `#![forbid(unsafe_code)]`；`unsafe` 代码、函数、trait 与实现均被禁止。
- `.yan`、密封 manifest 与 lock 是规范输入；生成的审查视图永远只读。
- 内容哈希绑定规范语义与制品，失败候选不能获得活动版本资格。

当前仍缺少独立进程级生产沙箱、正式权限系统、TLS 终止、成熟数据库适配、成熟编辑器生态、独立安全审计及长期兼容承诺。完整政策见 [SECURITY.md](SECURITY.md) 与 [Rust 安全策略](docs/engineering/rust-safety-policy.md)。

## 让 Codex / Claude Code / OpenCode 编写候选

仓库根目录的 [AGENTS.md](AGENTS.md) 和 [CLAUDE.md](CLAUDE.md) 会把代理引导到同一份[共享契约](docs/ai-agent-guide.md)。代理也可以作为隔离候选的编写后端：

```ps1
$env:YANSHU_PROVIDER = "codex-cli" # 也可使用 claude-code-cli / opencode-cli

cargo run --locked -p yanshu-cli -- `
  evolve-service `
  .runtime\tasks\code `
  examples\tasks\scenarios.json `
  --task .\TASK.md
```

代理只编辑一次性目录中的 `candidate.yan`，看不到真实活动指针、生产 capability 或可信测试文件。代理退出成功不等于候选通过；Parser、测试、fuel、内容哈希和人工晋升仍是最终证据。

已经在真实仓库中工作的 Agent 可以使用另一条只读路径：[`yanshu-mcp`](wiki/development/mcp.md) 接收当前完整源码文本并返回 inspection、格式化候选或审查投影。它没有文件、网络、执行和写回权限，不能替代 Agent 自己的编辑动作或候选门禁。

## 仓库地图

```text
rust/crates/
  yanshu-syntax       Reader、AST、Parser、语言版本门禁
  yanshu-format       注释保留、语义复核、幂等格式化
  yanshu-lsp          有界 stdio、诊断、hover、补全、语义高亮、导航、防捕获 rename、只读格式化 edit
  yanshu-mcp          Codex、Claude Code、OpenCode 的有界只读语言工具
  yanshu-runtime      解释器、Schema、Value、模式匹配、字节码 VM
  yanshu-analysis     类型、效果、capability 闭包、审查投影
  yanshu-compiler     规范字节码、verifier、WASM ABI
  yanshu-bundle       密封模块图与链接
  yanshu-package      内容寻址包与锁文件
  yanshu-library      Rust Library Backend 契约
  yanshu-cli          面向人类和 Agent 的稳定 JSON CLI

conformance/v1..v4    跨版本可执行语言契约
editors/vscode/       .yan 语言贡献、LSP client 与平台 VSIX 打包
editors/tree-sitter-yanshu/  容错增量 CST、标准查询与差分语料门禁
examples/             费用审批、任务服务、Bundle 与 package
docs/                 规范、安全和运维设计
wiki/                 面向使用者的中文语言 Wiki
```

## 项目状态

当前发布里程碑是 **v0.12**，语言版本是 **v4**。它已经是一个可执行、可分析、可编译，并带持续验证、Agent/LSP/MCP 与可验证安装包的语言内核，但还不是 Rust/C++ 式系统语言，也不是可承诺生产稳定性的通用平台。

v0.12 包含 formatter v1、稳定表达式节点路径、有界全局/局部符号索引、作用域感知 completion、全文 semantic tokens、同文档 definition/references、防捕获 rename、Rust 风格只读审查预览、平台专用 VS Code VSIX、Windows/Linux Extension Host 验收、带 corpus/查询/权威 Parser 差分门禁的 Tree-sitter grammar，以及兼容新旧协议的只读 MCP。接下来的工具优先级是更多编辑器安装包、semantic token range/delta 和结构化 AST diff；更广的标准库与有界结构化并发属于 v0.13+，并且只会在 capability、fuel、取消和确定性语义明确后加入。路线图是方向，不是兼容性承诺。

## 可验证发布

v0.11 建立了 CLI/MCP 证据链；当前 v0.12 发布链又纳入 `yanshu-lsp` 和平台 VSIX。Windows x86-64 与 Linux x86-64 job 会在两个独立 target-dir 构建三个 Rust 程序、执行各自真实 smoke 并逐字节比对，再用两份独立 LSP 逐次打包并比较 VSIX。确定性 ZIP、每平台构建记录、CLI/MCP/LSP/扩展四份 CycloneDX 1.5 SBOM、release manifest 与 `SHA256SUMS` 共同形成 schema v3 内容闭包。标签还必须是位于 `main`、与 workspace 版本完全一致的注解式标签。

发布资产由 GitHub OIDC 生成 keyless provenance，不在仓库保存签名私钥。下载后应同时检查内容闭包与来源证明：

```powershell
node scripts/verify-release.mjs <下载目录>
gh attestation verify <下载的资产> --repo Yukk1o/yanshu
```

当前承诺是同源码、同 runner 的双构建一致；托管 runner 与系统 linker 尚未 hermetic 固定，因此不夸大为“任意机器必然产生同一 hash”。完整边界见 [发布供应链说明](docs/engineering/release-supply-chain.md)。

## 参与开发

提交改动前请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [docs/ai-agent-guide.md](docs/ai-agent-guide.md)。任何语言特性都必须同步 Parser、解释器、VM、静态分析、诊断、conformance 和 Wiki；第一方 Rust 不接受任何形式的 `unsafe`。

完整发布门禁：

```ps1
cargo fmt --all -- --check
cargo test --workspace --locked -j 1
cargo clippy --workspace --all-targets --locked -j 1 -- -D warnings
cargo deny check
./scripts/check-repository-boundaries.ps1
cargo check --locked --manifest-path fuzz/Cargo.toml --bins
node --test scripts/release.test.mjs
node scripts/release-metadata.mjs
node scripts/check-doc-links.mjs

Push-Location editors\tree-sitter-yanshu
npm ci
npm audit --registry=https://registry.npmjs.org --audit-level=high
npm run check
Pop-Location

Push-Location wiki
npm run build
Pop-Location
```

项目采用 [MIT](LICENSE-MIT) 或 [Apache License 2.0](LICENSE-APACHE) 双许可证。
