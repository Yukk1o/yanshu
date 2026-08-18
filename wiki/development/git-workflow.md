# Git 分支工作流

项目把“运行时版本库”和 Git 分开使用：运行时版本库保存 LLM 候选证据；Git 保存经过人类审查、成为项目源码的演进历史。

## 长期分支

```text
main (v0.1.0)
  └─ develop
       ├─ feature/web-backend-runtime      已合并为 v0.2 检查点
       └─ feature/business-backend-v0.3   当前功能线
```

- `main`：始终是经过测试、可发布的检查点，并用 `v*` annotated tag 标记；
- `develop`：下一版本集成分支；
- `feature/<name>`：从 develop 分出的一项可独立审查能力；
- `hotfix/<name>`：从 main 分出，修复后同时合回 main 和 develop。

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

## 一项新能力怎样进入 develop

```powershell
git switch develop
git switch -c feature/example

# 修改 + 测试
.\scripts\test.ps1

git add <明确文件>
git commit -m "feat(...): ..."

git switch develop
git merge --no-ff feature/example
```

使用 `--no-ff` 保留功能边界。已经共享的历史不做 rebase/force-push；需要撤销时优先 revert commit。

## LLM 候选不等于 Git commit

运行时 candidate 存在 `.runtime/.../versions/<hash>.ail`，它可以是失败实验，也可能只用于本地验证。以下流程才会把候选变成仓库历史：

1. 候选通过可信 suite；
2. 人类按[AI 改动审查清单](/evolution/review-ai-change)核对业务意图与权限；
3. 把明确改动带入 feature branch；
4. 运行完整 `scripts/test.ps1`；
5. 创建可读 commit 并正常 review / merge。

候选 metadata 和模型 notes 不能代替 commit message，也不能代替代码审查。

## 不进入 Git 的内容

- `.toolchains/`；
- `.runtime/`；
- `.env` 与任何 API key；
- Racket `compiled/`、build output；
- `wiki/node_modules/`、VitePress cache/dist、npm cache；
- Wiki 构建时生成的 `public/source/` 快照。

Wiki 的 Markdown、配置、脚本和 lockfile 应进入 Git，以便构建可复现。
