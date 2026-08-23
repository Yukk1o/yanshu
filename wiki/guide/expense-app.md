# 费用审批实战

这个练习使用仓库内的 v4 费用审批 Bundle。你会运行一个多模块程序，阅读类型与 capability 分析，再安全地修改一条审批规则。

## 准备示例

这一页需要 Yanshu 仓库中的 `examples/`：

```powershell
git clone https://github.com/Yukk1o/yanshu.git
Set-Location yanshu
Copy-Item -Recurse examples\bundles\typed-expense .runtime\my-expense
```

将已安装 CLI 的绝对路径保存到一个变量：

```powershell
$yanshu = "C:\Tools\yanshu-v0.12.0-x86_64-pc-windows-msvc\yanshu.exe"
```

从源码运行时，可将后续 `& $yanshu` 命令换成 `cargo run --quiet --locked -p yanshu-cli --`。

## 认识两个模块

`policy.yan` 定义封闭的决策类型：

```lisp
(data decision
  (approved (amount integer))
  (review (amount integer) (reason string))
  (rejected (reason string)))
(export-types decision)
```

它还导出 `decide : Int -> decision`，并使用 `log` 记录决策。

`app.yan` 导入 policy，使用 `match` 把 Variant 转成 JSON 友好的 Map：

```lisp
(match decision
  ((approved value)
    (map "status" "approved" "amount" value))
  ((review value reason)
    (map "status" "review" "amount" value "reason" reason))
  ((rejected reason)
    (map "status" "rejected" "reason" reason))
  (_
    (map "status" "invalid")))
```

模块不是在运行时按路径随意搜索。`bundle.json` 固定了入口、语言版本、模块哈希和静态 capability 闭包。

## 先检查，再运行

```powershell
& $yanshu inspect-bundle .runtime\my-expense
& $yanshu review-bundle .runtime\my-expense --text
```

第一条命令的 JSON 中应有：

```json
{
  "capabilityClosure": ["log"],
  "declaredCapabilities": ["log"],
  "unusedCapabilities": []
}
```

只读审查视图会把任意精度 `Int`、只有 `#f` 为假的条件语义，以及 `audit!` / `log!` 副作用标记写在投影中。请用它帮助阅读，不要把它保存成 Rust 或回写 `.yan`。

运行示例参数 `[1200]`：

```powershell
& $yanshu run-bundle `
  .runtime\my-expense `
  evaluate `
  .runtime\my-expense\arguments.json
```

结果的业务部分是：

```json
{
  "status": "review",
  "amount": 1200,
  "reason": "manual approval required"
}
```

CLI 不会把 `log` 内容直接打印到终端，但会返回 `"logEvents": 1`。

## 修改审批阈值

在 `.runtime/my-expense/policy.yan` 找到：

```lisp
((>= amount 1000) (review amount "manual approval required"))
```

将阈值改为 `1500`。此时直接运行 Bundle 会因源码哈希与 manifest 不同而失败；这是预期保护。

格式化检查并重新密封：

```powershell
& $yanshu format .runtime\my-expense\policy.yan --check
& $yanshu seal-bundle `
  .runtime\my-expense `
  typed-expense `
  app.yan policy.yan
```

再次运行 `[1200]` 时，预期状态变为 `approved`。然后把 `arguments.json` 改成 `[1800]`，应再次得到 `review`。

## 为规则补测试

密封只证明“这个依赖闭包完整且分析通过”，不证明业务阈值正确。一个最小场景集应至少包含：

| 输入 | 预期 |
| --- | --- |
| `-1` | `rejected` |
| `0` | `approved` |
| `1499` | `approved` |
| `1500` | `review` |

这些边界值才是本次修改的业务意图。如果让 AI 修改规则，测试文件应留在可信宿主侧，AI 只提交候选 `.yan`。

## 进一步：变成 Web 业务服务

`examples/expenses/service.yan` 展示了同一领域的 Web 版本：

- `decision-request` Schema 白名单化 action、类别和外部引用；
- `list-map` / `sum` 计算总额；
- `list-filter` / `list-fold` 计算招待费；
- `checked-quotient` 在人数为零时走可控降级路径；
- `kv-put` 只在 handler 成功返回合法 response 时提交。

```powershell
cargo run --quiet --locked -p yanshu-cli -- `
  test-service `
  examples\expenses\service.yan `
  examples\expenses\scenarios.json
```

这个服务示例是 language v2 的业务回归用例，不是新项目的版本模板。新的多模块应用应优先使用 language v4 的签名、类型与效果门禁。

继续阅读 [Schema 与业务错误](/backend/schema-errors)和 [Web DSL 与路由](/backend/web)。
