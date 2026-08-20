# Yanshu v0.11：持续验证与不可信输入测试

状态：实施中。v0.11 不改变 `.yan` language version v4，也不增加 guest capability；它把现有信任边界变成每个提交都必须重复证明的自动化契约。

## 1. 目标

v0.11 的第一阶段建立三条独立证据链：

1. 每次 push / pull request 在 Windows 与 Linux 上执行 Rust 格式、测试和 Clippy；
2. 在 Linux 上执行第一方 safe Rust、凭据模式、cargo-deny、v1-v4 conformance、编译产物与标准 WebAssembly 引擎 smoke test；
3. 定时和按需使用 libFuzzer 攻击 Reader/Parser、portable JSON Value、bytecode/WASM artifact loader。

CI 不是新的可信解释器，也不赋予 AI 或 pull request 晋升权。它只能拒绝缺少证据的变更，不能证明程序没有缺陷。

## 2. 工作流边界

`.github/workflows/ci.yml` 使用只读 `contents` 权限，不持久化 checkout 凭据。第三方 Action 固定到完整 commit SHA；升级 Action 必须作为普通依赖变更审查，不能只移动 tag。

CI 分为四个 job：

- `rust`：Windows/Linux 的 fmt、workspace test、Clippy `-D warnings`；
- `security`：第一方源码边界、已跟踪凭据模式和 cargo-deny；
- `conformance`：v1-v4 manifest、typed Bundle 编译与 WASM ABI；
- `wiki`：`npm ci`、高危依赖审计和 VitePress build。

工作流不读取 provider key，不运行 Agent Backend，不启动公网服务，也不晋升候选版本。

## 3. Fuzz 契约

`fuzz/` 是独立 Cargo workspace，避免把 libFuzzer 运行时加入语言或发布二进制的依赖图；它有自己的 lock 和 cargo-deny 检查。`libfuzzer-sys` 的 `(MIT OR Apache-2.0) AND NCSA` 许可证被显式记录，不能借 fuzz 依赖扩展发布二进制的依赖面。三个 target 都是 safe Rust，并复用正式公开入口：

| Target | 不可信输入 | 失败条件 |
| --- | --- | --- |
| `reader_parser` | 任意 UTF-8 source bytes | panic、abort、越界或 sanitizer 报告 |
| `portable_value` | 任意 JSON bytes | portable value 转换崩溃或 sanitizer 报告 |
| `artifact_loaders` | 任意 artifact bytes | bytecode/WASM loader 崩溃或 sanitizer 报告 |

预期的结构化 `Diagnostic` 不是 fuzz 失败。CI 限制单输入长度、RSS 和总运行时间；发现的 crash artifact 只短期保留，人工最小化后应转成普通回归测试。不得把包含业务数据或凭据的 corpus 上传到仓库。

libFuzzer 需要 nightly、LLVM sanitizer 和 Unix-like 主机。Windows 开发者使用稳定 Rust 执行编译检查，实际 fuzz 由 Linux CI 或 Linux 本地环境运行。

## 4. 本地命令

稳定工具链检查 fuzz target 能否编译：

```powershell
cargo check --locked --manifest-path fuzz/Cargo.toml --bins
```

在安装 `cargo-fuzz 0.13.2` 的 Linux nightly 环境运行短 smoke：

```bash
cargo fuzz run reader_parser -- -max_total_time=30 -max_len=1048576 -rss_limit_mb=2048
cargo fuzz run portable_value -- -max_total_time=30 -max_len=1048576 -rss_limit_mb=2048
cargo fuzz run artifact_loaders -- -max_total_time=30 -max_len=1048576 -rss_limit_mb=2048
```

源码边界检查：

```powershell
./scripts/check-repository-boundaries.ps1
```

## 5. 未完成范围

v0.11 后续仍需补齐可复现的 Windows/Linux release archive、SHA-256、SBOM、签名策略、长期 fuzz corpus 治理，以及针对 Bundle/package 文件系统加载器和 HTTP parser 的隔离 fuzz harness。LSP、formatter 和 MCP 属于 v0.12；标准库扩展与有界结构化并发属于 v0.13 以后。
