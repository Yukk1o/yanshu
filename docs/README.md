# Yanshu 工程文档

这里保存实现者、审计者和运维者使用的工程契约。面向语言使用者的语法、范式、标准库和教程请从 [Wiki](../wiki/README.md) 进入；根目录 [README](../README.md) 提供项目总览。

## 当前文档

| 目录 | 内容 | 权威性 |
| --- | --- | --- |
| [`ai-agent-guide.md`](ai-agent-guide.md) | Codex、Claude Code、OpenCode 与其他代理的共享仓库契约 | 当前、强制 |
| [`specs/`](specs/) | v0.6 至 v0.11 的语言与里程碑契约、审计收口 | 当前规格 |
| [`engineering/`](engineering/) | safe Rust、依赖、发布供应链、Git 与 VersionStore 设计 | 当前工程契约 |
| [`operations/`](operations/) | Provider、影子运行、备份与恢复 | 当前操作边界 |
| [`guides/`](guides/) | 迁移类指南 | 当前指导 |
| [`archive/`](archive/) | v0.1 至 v0.5 原型阶段材料 | 历史、非规范 |

Rust 是唯一受支持的宿主实现。旧原型源码和运行脚本已从当前仓库树移除；`archive/` 只解释设计演进，不能作为现行行为、命令或兼容性依据。

## 维护规则

- 每份工程文档必须能从本索引或其目录索引找到，并明确当前或归档状态。
- 当前行为优先以 `.yan` 规格、conformance、Rust 实现和 Wiki 为准；历史文档不得覆盖它们。
- 文件移动后必须同步根文档、内部链接、`wiki/scripts/sync-source.mjs` 和 Wiki 源码地图。
- 不在 `docs/` 保存生成镜像、运行数据、凭据或聊天记录。
