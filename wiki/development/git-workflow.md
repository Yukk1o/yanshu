# Git 分支工作流

项目把“运行时版本库”和 Git 分开使用：运行时版本库保存 LLM 候选证据；Git 保存经过人类审查、成为项目源码的演进历史。

## 当前分支模型

```text
main
  ├─ feature/v0.11-ci-security          PR 验证后合并
  ├─ feature/v0.11-release-supply-chain 独立审查发布链
  └─ release/yanshu-v0.10               历史检查点
```

- `main`：唯一发布来源，始终是经过测试的检查点；
- `feature/<name>`：从最新 main 分出的一项可独立审查能力，通过 PR 合回 main；
- `release/<name>`：需要时保留历史检查点，不是第二发布来源；
- `hotfix/<name>`：从 main 分出，仍通过正常 PR 与门禁合回 main。

旧 `develop` 只保留为历史，不再是当前集成分支。这样 PR、branch policy 与 release provenance 只有一个可信根。

真实规则：[docs/git-workflow.md](/source/docs/git-workflow.md.txt)。

## 当前历史表达了什么

```text
v0.1 受限 Lisp + 版本化 LLM 候选闭环
  │
  ├─ v0.2 route/capability → 事务 KV → HTTP CRUD + 测试门禁
  │
  └─ v0.3 compiler-owned Schema + 统一 API 错误
```

这些能力拆成 specification、runtime、example、tests、docs 等内聚提交，便于判断语义从哪一次开始改变。

## 一项新能力怎样进入 main

```powershell
git switch main
git pull --ff-only
git switch -c feature/example

# 修改 + 测试
cargo test --workspace --locked

git add <明确文件>
git commit -m "feat(...): ..."

# 推送 feature 分支，创建 PR；所有门禁通过后再合并到 main
```

已经共享的历史不做 rebase/force-push；需要撤销时优先 revert commit。发布标签必须是与 workspace 版本完全一致的 annotated tag，且指向 main 已包含的 commit；资产只能由[可验证发布](/development/releases)工作流生成，不能手工替换。

## LLM 候选不等于 Git commit

运行时 candidate 存在 `.runtime/.../versions/<hash>.yan`，它可以是失败实验，也可能只用于本地验证。以下流程才会把候选变成仓库历史：

1. 候选通过可信 suite；
2. 人类按[AI 改动审查清单](/evolution/review-ai-change)核对业务意图与权限；
3. 把明确改动带入 feature branch；
4. 运行 Rust workspace 测试、语言 conformance 与相关业务 suite；
5. 创建可读 commit 并正常 review / merge。

候选 metadata 和模型 notes 不能代替 commit message，也不能代替代码审查。

## 不进入 Git 的内容

- `.runtime/`；
- `.env` 与任何 API key；
- Rust `target/`、Wiki build output；
- `wiki/node_modules/`、VitePress cache/dist、npm cache；
- Wiki 构建时生成的 `public/source/` 快照。

Wiki 的 Markdown、配置、脚本和 lockfile 应进入 Git，以便构建可复现。
