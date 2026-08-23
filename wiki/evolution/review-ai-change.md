# 如何审查 AI 生成的改动

你不需要先流利阅读 Lisp，才能判断一个 AI 候选是否值得晋升。审查顺序应该从**业务意图和外部契约**开始，最后才进入括号里的实现细节。

## 先接受一个边界

测试通过只证明“候选满足了现有测试描述的行为”，不证明：

- 测试完整表达了你的业务意图；
- 没有遗漏边界情况；
- 权限扩大是合理的；
- 数据格式变化可以安全回滚；
- 错误信息适合真实用户；
- 新逻辑在测试规模之外仍有合理性能。

因此测试是晋升的**必要条件**，不是业务审查的替代品。AI 不能给自己判通过，人也不能只看到绿色测试就停止思考。

## 非 Lisp 读者的五步审查法

### 1. 先读业务场景差异

从 JSON suite 开始，因为它最接近需求：

```json
{
  "name": "reject-duplicate-id",
  "method": "POST",
  "path": "/tasks",
  "body": {"id": "business-1", "title": "duplicate"},
  "expectStatus": 409,
  "expectBodyContains": {
    "error": {"code": "TASK_EXISTS"}
  }
}
```

逐项问：

- 新需求对应的新场景在哪里？
- 成功、失败、权限不足、重复请求和资源不存在是否都覆盖？
- 场景是否只验证了状态码，却漏掉关键数据？
- 有状态操作是否验证了后续读取和删除？
- AI 有没有删除、放宽或改写原有期望？

当前 suite：[任务业务场景](/source/examples/tasks/scenarios.json.txt)。真正的审查规则应禁止候选修改可信测试；如果人类要改 suite，应作为单独、可解释的评审内容。

### 2. 审 Schema：允许什么数据进入系统

找到候选中的 `(schema ...)`，按结构读，不必理解函数式语法：

```lisp
(schema task-create
  (object
    (required "id" (string 1 64))
    (required "title" (string 1 120))
    (optional "completed" boolean #f)))
```

把它直接翻译成 Rust 风格心智模型：

```rust
struct TaskCreate {
    id: BoundedString<1, 64>,
    title: BoundedString<1, 120>,
    completed: bool, // default false
}
```

审查问题：

- required 是否被改成 optional？
- 最大长度或整数范围是否被异常放大？
- 默认值是否改变业务含义？
- 是否新增/删除字段，旧客户端和旧数据怎么办？
- 封闭对象拒绝额外字段的约束是否仍成立？

Schema 的变化就是 API 输入面变化，应当像 Go/Rust DTO 变化一样认真看待。

### 3. 审路由与能力：程序能触达什么

路由是程序的公开入口：

```lisp
(route DELETE "/tasks/:id" delete-task)
```

能力是程序的权限清单：

```lisp
(capabilities kv clock log)
```

审查 diff 时优先寻找：

- 新增了哪些 method + path？
- handler 是否意外从只读变成写入？
- 是否删除或重命名既有路由？
- `capabilities` 是否新增权限？为什么需要？
- 纯函数候选为什么突然需要 `kv` 或 `clock`？

能力扩大应该像 Rust crate 获得新的 trait implementation，或 Go 服务获得新的 IAM 权限一样单独说明。当前只有 `kv`、`clock`、`log`，未来出现网络、文件、邮件等能力时更要采用默认拒绝。

### 4. 审统一错误：客户端看到什么

搜索所有 `api-error`：

```lisp
(api-error 409 "TASK_EXISTS" "task id already exists")
```

审查：

- status 是否符合业务语义；
- 稳定 `code` 是否被无理由更名；
- public message 是否泄漏内部 key、堆栈或敏感数据；
- `details` 是否稳定、有限且可公开；
- 同一失败是否在不同 handler 返回不同 code。

客户端通常依赖 `error.code`，改名等同于破坏 API contract，不是普通文案修改。

### 5. 最后审版本差异和关键 Handler

版本库按 SHA-256 保存完整源码，因此应比较父版本与候选版本，而不是只读模型给出的 notes。notes 是不可信说明，diff 才是事实。

在 Lisp diff 中先抓这些锚点：

```text
(capabilities ...)  权限
(schema ...)        输入契约
(route ...)         公开入口
(def handler-name   业务实现
(kv-put ...)        写入
(kv-delete ...)     删除
(api-error ...)     错误契约
(export ...)        可调用表面
```

对于 handler，把嵌套括号按下面方式读：

| Lisp | 先翻译成 |
| --- | --- |
| `(let ((x value)) body)` | `let x = value; body` |
| `(if cond yes no)` | `if cond { yes } else { no }` |
| `(do a b c)` | `{ a; b; c }`，返回 `c` |
| `(get map "x")` | `map["x"]` |
| `(kv-get key #f)` | `tx.get(key).unwrap_or(false)` |
| `(api-response 200 x)` | `Ok(HttpResponse::json(200, x))` 的外观 |

不确定某段逻辑时，不要因“AI 写的而且测试通过”而默认接受；要求新增能表达你疑问的业务场景，再重新运行完整 suite。

## 晋升前检查表

- [ ] 我能用一句中文说明这次业务变化。
- [ ] 新旧业务场景的差异与这句话一致。
- [ ] 原有场景没有被删除或放宽。
- [ ] Schema required/optional、边界和默认值合理。
- [ ] 路由变化是预期的，没有意外扩大 API。
- [ ] capability 没有扩大，或扩大有明确理由与宿主限制。
- [ ] `api-error` 的 status/code/message/details 保持稳定、可公开。
- [ ] 写入和删除路径有成功、失败与回滚场景。
- [ ] 完整 suite 通过，不只是失败案例通过。
- [ ] 候选已注册但尚未自动晋升；晋升是一次独立决定。
- [ ] 我知道如何回滚代码，也理解数据变化是否可逆。

## 当前已有的审查工具

- `check` / `inspect`：输出 Parser 看到的完整 program AST；
- `review` / `review-bundle --text`：生成 `rust-readonly-v3` 单向语义投影；
- VS Code **打开 Rust 风格只读审查**：从当前打开快照显示无脚本旁侧面板；
- JSON test report：输出每个失败的 name、reason、expected、actual；
- content-addressed source：保留父版本和候选完整源码；
- metadata / events：记录 parent、provider、报告、promote 和 rollback；
- `git diff`：审查进入项目源码树的人类/AI 修改。

CLI 用法见 [CLI 参考](/reference/cli)。

## Rust 风格只读审查视图

为了让不懂 Lisp 的人更安全地审查，Rust 分析器会从同一份已验证 AST **生成单向语义视图**：

```rust
// Generated semantic review — READ ONLY.
// This is not Rust source and cannot be executed.
// semantic Int = arbitrary-precision integer (never i32/i64).

fn audit(value: Int) -> Int {
    { log!(value); value }
}
```

当前视图满足：

1. 从 AST 单向生成，用户不能编辑它再反向执行；
2. definition 节点携带原 `.yan` source span、类型和 capability；
3. 显式标注任意精度 `Int`、truthiness 与 `log!` 一类效果调用；
4. VS Code 面板无 `TextDocument`、无脚本、无执行或保存入口；
5. 仍以 `.yan` AST、测试和宿主策略作为执行真相。

当前还没有 route/schema/错误码的结构化差异视图，也没有从投影回写 AST 的编辑协议。它的价值是降低审查门槛，不是创建第二门可执行语言。实际使用方式见 [VS Code 使用指南](/development/vscode)。
