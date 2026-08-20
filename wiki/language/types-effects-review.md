# 类型、效果与 Rust 风格只读审查

v4 在“代码能解析”与“代码能运行”之间增加强制静态门禁：导出 API 有明确类型，内部定义接受推断；每个 export 的 capability 闭包在运行前计算。分析结果同时驱动只读审查视图。

## 导出签名

```lisp
(signature evaluate (fn (integer) map))

(def evaluate
  (fn (amount)
    ...))

(export evaluate)
```

v4 导出的 definition 缺少 signature 会在 Parser 阶段拒绝。构造器的函数类型由 `data` 字段直接确定，不重复声明。

当前类型包括：

- `integer`、`boolean`、`string`、`symbol`、`nil`、`map`；
- 用户定义的名义类型；
- `(list T)` 与 `(result T E)`；
- `(fn (T ...) R)`；
- 明确的渐进边界 `any`。

`any` 主要出现在 JSON Map lookup、Schema union 等动态边界。它不会关闭运行期检查：宿主参数在执行前按 signature 验证，guest 结果在返回前再次验证。

## typed data field

```lisp
(data decision
  (approved (amount integer))
  (review (amount integer) (reason string))
  (rejected (reason string)))

(export-types decision)
```

v3 的字段只有名字；v4 的每个字段都必须有类型。模式构造器、字段数量和分支结果会参与统一，错误带回原 `.ail` span。

值和类型使用两个显式导出表：`export` 控制函数与构造器，`export-types` 控制其他模块能否在签名中写这个名义类型。Bundle 链接器只从直接 `imports` 解析类型，把 `decision` 固定成 `typed-policy/decision`；未导出、找不到或多个依赖同名都会拒绝密封。

## capability 闭包不是手填结果

```lisp
(capabilities log)
```

这仍是允许上限，不是分析结论。分析器从 export 出发，穿过普通调用、递归、模块 import 和已知高阶 callback，计算真正可达的 `log / kv / clock`：

```json
{
  "capabilityClosure": ["log"],
  "declaredCapabilities": ["log"],
  "unusedCapabilities": []
}
```

漏声明会失败；多声明会进入 `unusedCapabilities`。如果导出 API 调用了一个无法解析其来源的函数参数，分析器返回 `EFFECT_UNRESOLVED_PARAMETER`，不会把未知效果当作纯函数。

## Bundle format v2

v4 的 `bundle.json` 把排序后的 `capabilityClosure` 放进内容哈希。加载器不会信任它：每次加载都重新解析、链接、分析并比对。源码、依赖、入口、类型或 effect 改变都会形成新的 Bundle ID。

## Rust 风格审查视图

```powershell
cargo run --locked -p ail-cli -- `
  review-bundle examples\bundles\typed-expense --text
```

核心输出类似：

```rust
// Generated semantic review — READ ONLY.
// semantic Int = arbitrary-precision integer (never i32/i64).
// semantic truthy(value) = false only for Bool(false).
// calls spelled name!(...) directly or transitively perform capability effects.
// capability closure: [log]

enum TypedPolicyDecision {
    Approved { amount: Int },
    Review { amount: Int, reason: String },
    Rejected { reason: String },
}

// source: typed-policy:20:5 | effects: [log]
fn typed_policy__decide(amount: Int) -> TypedPolicyDecision {
    typed_policy__audit!(
        if truthy((amount < 0)) {
            TypedPolicyDecision::Rejected { reason: "negative amount" }
        } else {
            TypedPolicyDecision::Approved { amount: amount }
        }
    )
}
```

`!` 是审查标记，不是第二套可执行语法。`log!(...)` 表示该行直接调用 capability；`typed_policy__audit!(...)` 表示调用会传递性到达 capability。机器节点仍保留精确 `.ail` span 和 capability 列表。

机器可读 node 还带 definition ID、逻辑模块、起止行列、推断类型与 capability。它适合审查，不是第二份源码：

- `renderer` 固定为 `rust-readonly-v3`；
- `editable` 永远是 `false`；
- 文本不能交给 CLI 执行，也没有反向 Parser；
- `.ail` AST、签名、Bundle 和测试仍是唯一真相。

不加 `--text` 时，CLI 保留包含 analysis、node 与 `text` 字段的单行 JSON，供自动化工具消费；加上 `--text` 才直接打印带缩进的审查文本。两种模式不改变只读边界。

“视图 + 结构化编辑”已明确推迟到 v0.10 之后。当前不会为了省一次 LLM 调用而提前建立不可靠的双向转换。

完整契约见 [v0.8 规格](/source/docs/spec-v0.8.md.txt)，实现见 [类型推断](/source/rust/crates/ail-analysis/src/infer.rs.txt)、[效果分析](/source/rust/crates/ail-analysis/src/effects.rs.txt)与[只读 renderer](/source/rust/crates/ail-analysis/src/review.rs.txt)。
