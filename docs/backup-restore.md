# Rust 服务备份与恢复

Rust host 提供离线、可校验、默认拒绝覆盖的服务快照。目标不是把若干文件“复制一份”，
而是让操作者在恢复前后都能证明版本库、活动指针、事件历史和业务 KV 仍然自洽。

## 命令

```powershell
# 服务停止后创建快照；目标目录必须不存在
cargo run --locked -p yanshu-cli -- backup-service `
  .runtime\tasks-rust\code `
  .runtime\tasks-rust\store.json `
  .backups\tasks-2026-08-18

# 可在另一台机器或发布前重复执行，只读校验
cargo run --locked -p yanshu-cli -- verify-backup `
  .backups\tasks-2026-08-18

# 恢复目标必须都不存在，不会覆盖现有服务
cargo run --locked -p yanshu-cli -- restore-service `
  .backups\tasks-2026-08-18 `
  .runtime\tasks-restored\code `
  .runtime\tasks-restored\store.json
```

成功结果是结构化 JSON，包含活动版本、文件数、总字节数和 KV 文件是否存在。恢复完成后应
先运行 11 个业务场景并在新的 loopback 端口启动，再由操作者决定是否切换流量。

## 快照包含什么

```text
snapshot/
├─ manifest.json
└─ payload/
   ├─ code/
   │  ├─ active.json
   │  ├─ events.jsonl
   │  ├─ versions/<sha256>.yan
   │  └─ metadata/<sha256>.json
   └─ data/store.json       # 原服务尚无写入时可以不存在
```

`manifest.json` 使用 schema v1，为每个 payload 文件保存相对路径、字节数和 SHA-256。
观测 JSONL 不属于业务恢复点，不进入这个快照；生产部署应由日志采集器单独轮转、传输和设置
保留期。

## 失败关闭边界

- `yanshu-server` 在整个进程生命周期持有 `<data-store>.service.lock`；运行中的服务会让离线
  backup/restore 返回 `SERVICE_MAINTENANCE_LOCKED`。
- backup 同时持有版本库的 `.yanshu-store.lock`，避免候选注册或活动指针切换发生在快照中间。
- 源码文件名、源码内容 SHA-256、metadata、活动指针以及 registered/promoted/rolled-back
  事件序列必须形成完整生命周期。
- KV 必须是当前 v1 文档，符号链接、未知版本库文件、manifest 外文件、路径穿越、超限文件、
  重复路径和 hash/size 不一致都会被拒绝。
- snapshot 目标、恢复后的 code store 和 data store 必须不存在。实现使用 `create_new` 写文件，
  不提供覆盖开关。
- 第一方实现继承 `unsafe_code = "forbid"`，文件锁、读写和 SHA-256 都只通过安全 API。

## 当前限制

这是单机文件后端的**离线**恢复点，不是在线数据库快照。它不包含操作系统权限、TLS 配置、
反向代理配置、provider 密钥或观测日志，也不替代异地复制、加密、签名和定期恢复演练。
未来数据库后端应使用数据库自身的一致性快照和 WAL/PITR，再把活动代码版本写入同一恢复清单。
