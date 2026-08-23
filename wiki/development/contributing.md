# 参与衍术开发

这份 Wiki 面向使用语言的开发者。如果你要修改 Parser、运行时、编译器、LSP 或发布流程，请改用仓库内的维护者文档：

1. 先完整阅读 [`docs/ai-agent-guide.md`](/source/docs/ai-agent-guide.md.txt)；
2. 再按改动范围阅读 [`docs/specs/`](/source/docs/specs/v0.12.md.txt) 中对应的语义规范；
3. 使用仓库 `docs/engineering/` 和 `docs/operations/` 的工程、发布与运维契约。

语言实现有几条不可放宽的边界：第一方 Rust 只允许 safe Rust；不得向 guest 增加 `eval`、隐式宿主访问、未计量工作或未声明 capability；语法版本门禁、解释器/VM 语义、诊断、内容哈希和 capability 分析必须同步。

Wiki 不记录 PR 流水账、crate 边界快照或版本里程碑过程。这些信息应留在 GitHub、规范或维护者文档中；Wiki 只教开发者如何理解和使用已经实现的语言。
