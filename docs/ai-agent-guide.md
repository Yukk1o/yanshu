# Yanshu：AI 编程代理指南

这份文件同时定义两件事：Codex、Claude Code、OpenCode 如何参与开发这个仓库，以及 Yanshu 如何把它们作为候选程序的 Agent Backend 调用。根目录的 `AGENTS.md` 与 `CLAUDE.md` 只负责自动发现；项目约束只在这里维护，避免不同代理得到互相矛盾的规则。

## Agent Backend：让工具实际编写候选程序

`evolve-service` 不只支持直接调用 OpenAI/DeepSeek HTTP API，也能启动用户已经安装并登录的编程代理 CLI：

```powershell
$env:YANSHU_PROVIDER = "codex-cli"       # 或 claude-code-cli / opencode-cli
cargo run --locked -p yanshu-cli -- `
  evolve-service `
  .runtime\tasks\code `
  examples\tasks\scenarios.json `
  --task <task.md>
```

调用链是：

```text
active 源码 + 可信测试报告
              ↓
一次性候选目录（candidate.yan / OBSERVATIONS.json / TASK.md）
              ↓
Codex / Claude Code / OpenCode 非交互编辑
              ↓
Rust 宿主重新读取并限制大小、拒绝 symlink
              ↓
Parser → 完整 suite → 内容哈希 → 注册 → 可选人工授权晋升
```

代理看不到真实 code store、suite 文件、active 指针或生产 capability。Codex 使用 `workspace-write` 且关闭网络；Claude Code 只允许 Read/Edit/Write，禁用 Bash 与 Web；OpenCode 使用内联 deny-by-default permission。宿主不会把名称包含 key/token/secret/password/credential 的环境变量传给子进程，所以这些工具应先通过各自安全凭据存储完成登录，而不是依赖当前 shell 的 API key。

这是一条本地开发后端，不是生产沙箱。第三方 agent CLI 本身属于宿主侧高权限软件，仍应在操作系统容器或独立低权限账户中运行；它永远不能替代 Parser、fuel、测试、内容哈希和晋升门禁。

## 1. 先理解项目

Yanshu 是“程序即数据”的受限通用语言内核。AI 可以生成候选程序，但候选必须经过 Parser、类型/效果分析、capability 闭包、fuel、Schema、内容哈希和测试门禁，才允许执行或晋升。

当前语言版本是 v4，已发布里程碑是 v0.10，v0.11 持续验证与安全加固正在开发。Rust 是唯一主实现；第一方 crate 与 fuzz target 均要求 `#![forbid(unsafe_code)]`。

核心信任边界：

- guest 没有 `eval`、任意宿主调用、文件、网络、线程或动态库入口；
- 宿主能力必须显式声明、注入、静态分析并在运行时复核；
- 所有跨边界值必须是有深度、节点数、字节数和整数位数上限的 portable value；
- 解释器和字节码 VM 必须保持相同语言语义及 fuel 耗尽边界；
- `.yan` 源码、Bundle manifest、package source 与 lock 是规范输入；生成的 Rust 风格审查文本不是 Rust 源码，也不能反向执行。

## 2. 仓库地图

- `rust/crates/yanshu-syntax`：Reader、AST、版本门禁、Parser、稳定节点路径和有界全局/局部符号索引。
- `rust/crates/yanshu-format`：保留注释、验证语义不变且幂等的 formatter。
- `rust/crates/yanshu-lsp`：有界 stdio LSP、文档快照、精确 token hover、作用域感知补全、导航、防捕获 rename、格式化 edit 与版本化只读审查请求。
- `editors/vscode`：`.yan` 语言贡献、无脚本只读审查面板、受信 server 选择、环境脱敏和平台专用 VSIX 打包。
- `rust/crates/yanshu-runtime`：解释器、portable value、Schema、pattern 和字节码 VM。
- `rust/crates/yanshu-analysis`：静态类型、效果、capability 闭包与只读审查投影。
- `rust/crates/yanshu-compiler`：规范字节码、verifier、artifact 和 WASM handle ABI。
- `rust/crates/yanshu-bundle`、`yanshu-package`：密封模块链接、内容寻址包与锁文件。
- `rust/crates/yanshu-library`：有版本、有契约、有计费规则的 Rust Library Backend。
- `rust/crates/yanshu-http`：按 transport/request/auth/loader/router/dispatch/response/observation/shadow 划分的宿主 HTTP 信任边界；`lib.rs` 只导出公共 API。
- `rust/crates/yanshu-store`：内容寻址版本、恢复 journal、active pointer 与完整性链事件；`lib.rs` 只保留公共 API 和稳定诊断。
- `rust/crates/yanshu-cli`：面向人类和代理的稳定 JSON CLI。
- `conformance/v1` 至 `conformance/v4`：跨版本可执行语言契约。
- `examples/`：任务、费用审批、typed Bundle 与 package 场景。
- `docs/specs/v0.6.md` 至 `docs/specs/v0.12.md`：各里程碑的当前规范。
- `.github/workflows/release.yml` 与 `scripts/*release*.mjs`：版本绑定、双构建、确定性归档、SBOM、校验和与来源证明。
- `wiki/`：面向使用者的语言 Wiki；`wiki/public/source/` 由同步脚本生成，禁止手改。

## 3. 阅读和编写 `.yan`

先看 `wiki/guide/quickstart.md`、`wiki/language/syntax.md` 和最接近需求的 `examples/`。一个 v4 模块大致如下：

```lisp
(program
  (name expense-policy)
  (version 4)
  (capabilities log)

  (data decision
    (approved (amount integer))
    (review (amount integer) (reason string)))
  (export-types decision)

  (signature decide (fn (integer) decision))
  (def decide
    (fn (amount)
      (cond
        ((< amount 0) (review amount "negative amount"))
        (else
          (do
            (log amount)
            (approved amount))))))

  (export decide approved review))
```

不要凭熟悉的 Lisp、Rust 或 JavaScript 猜语义：

- 只有 `Bool(false)` 为假；`0`、空字符串、空列表和 `Nil` 都为真；
- `Int` 是有运行时位数上限的任意精度整数；
- `and`、`or`、`if` 和 `cond` 短路；求值顺序是从左到右；
- capability 调用在审查视图中显示为 `log!` 一类效果标记；源码中仍按 `.yan` 语法书写；
- 新语法必须有 language-version 门禁；旧版本不能静默接受它；
- 不允许用新内建、宿主捷径或通用异常绕开 capability、Result 或 fail-loud 约束。

## 4. 修改协议

1. 先定位规范、现有测试和调用链，再做最小闭合改动。
2. 语法或语义变化必须同时更新 Parser、运行时、静态分析、审查投影、解释器/VM 差分测试、conformance、CLI 与 Wiki。
3. 新的循环、复制、解析、序列化、宿主返回值或字符串/整数运算必须在昂贵工作前检查边界并计入 fuel。
4. 新 capability 必须有声明、效果、arity、输入输出包络、fuel 成本、无宿主错误和测试；不能静默降级。
5. 保持稳定机器错误码。可以改进人类消息，但不要让代理只能解析自由文本。
6. 内容寻址数据必须使用规范序列化；开发路径、时间和无序 map 迭代不得污染语义哈希。
7. 不记录密钥、认证头、原始 provider 配置或未脱敏业务数据。仓库中禁止真实 token。
8. 不编辑不属于当前任务的用户改动，不用生成文件覆盖手写源码。

发布供应链也属于信任边界：只允许与 workspace 版本一致、位于 `main` 的注解式标签触发发布；pull request 和手动演练没有发布权限。不要手工替换 Release 资产、把 checksum 当作签名、加入长期私钥，或放宽双构建、SBOM、manifest 与 provenance 闭包。完整契约见 `docs/engineering/release-supply-chain.md`。

## 5. 给代理使用的稳定命令

读取单文件、Bundle 的机器报告或只读文本：

```powershell
cargo run --locked -p yanshu-cli -- inspect examples\expenses\service.yan
cargo run --locked -p yanshu-cli -- format examples\expenses\service.yan --check
cargo run --locked -p yanshu-cli -- review-bundle examples\bundles\typed-expense
cargo run --locked -p yanshu-cli -- review-bundle examples\bundles\typed-expense --text
cargo run --locked -p yanshu-lsp
```

运行 v1 至 v4 的可执行契约：

```powershell
cargo run --locked -p yanshu-cli -- conformance conformance\v1\manifest.json
cargo run --locked -p yanshu-cli -- conformance conformance\v2\manifest.json
cargo run --locked -p yanshu-cli -- conformance conformance\v3\manifest.json
cargo run --locked -p yanshu-cli -- conformance conformance\v4\manifest.json
```

编译与执行命令见 `docs/specs/v0.10.md` 和 `wiki/reference/cli.md`。CLI 默认输出稳定 JSON；需要人类审查时才使用 `--text`。不要解析 Cargo 的显示文本代替 CLI JSON。

## 6. 完成前门禁

按改动范围先运行目标 crate 测试。声称语言或发布里程碑完成前，在仓库根目录运行：

```powershell
cargo fmt --all -- --check
cargo test --workspace --locked -j 1
cargo clippy --workspace --all-targets --locked -j 1 -- -D warnings
cargo deny check
./scripts/check-repository-boundaries.ps1
cargo check --locked --manifest-path fuzz/Cargo.toml --bins
node --test scripts/release.test.mjs
node scripts/release-metadata.mjs
node scripts/check-doc-links.mjs
```

再确认第一方源码没有 `unsafe`，并在 `wiki/` 运行：

```powershell
npm run build
```

若改动 `editors/vscode`，还必须在该目录运行 `npm ci`、`npm run check` 和官方 registry 的 `npm audit`。生成本机 VSIX 前先在仓库根目录运行 `cargo build --locked --release -p yanshu-lsp`，再运行 `npm run package`；产物必须只包含当前平台 server、bundle 后 client、语言贡献、主/第三方许可证和对应 manifest。

若变更影响解释执行、编译执行、类型/效果或版本语义，还必须运行相应 conformance，并证明解释器与 VM 对结果、错误和 fuel 边界一致。测试通过不代表可以降低规格中的安全红线。

## 7. 当前支持边界

现在同时支持两条路径：代理进入真实仓库参与语言实现；以及 `evolve-service` 在一次性候选目录中调用 Codex/Claude Code/OpenCode 编写一个 `.yan` 候选。后者默认只登记，不晋升，且不会把 agent 的退出状态或 notes 当成通过证据。

当前 formatter v1 已提供只读候选输出、CI check 和不依赖 source offset 的表达式节点路径；`yanshu-syntax` 已提供全局 `def`、参数、顺序 `let`、pattern binding 和嵌套遮蔽的有界符号索引；最小 `yanshu-lsp` 已提供 full sync、诊断、关键字/原语/Library/用户绑定的精确 plaintext hover、作用域/版本/capability 感知 completion、同文件全局/局部 definition、references、防捕获 rename、只读 formatting edit 和版本化 `yanshu/reviewDocument`；VS Code 扩展已提供 `.yan` 注册、基础 TextMate 高亮、无脚本只读审查面板、平台专用 VSIX，以及隔离的 Windows/Linux Extension Host 验收。尚未提供 Tree-sitter grammar、semantic tokens、其它编辑器安装包、MCP server、LSP 局部增量或审查视图的结构化回写；这些不能用不可靠的文本反向转换冒充。
