# 数据模型

Yanshu 的值系统刻意保持小而可移植。值既要能被解释器执行，也要能跨 JSON、版本库和 Library Backend 安全传递。

## 值类型总览

| `.yan` 类型 | 示例 | Rust 心智模型 | 可作为 JSON 输出 |
| --- | --- | --- | --- |
| Nil | `'()`；输入 JSON `null` 或 `[]` | `Value::Nil` | 是，统一编码为 `[]` |
| Bool | `#t`、`#f` | `Value::Bool(bool)` | 是 |
| Int | `42`、`9223372036854775808` | `Value::Int(BigInt)` | 是 |
| String | `"task-1"` | `Value::String(String)` | 是 |
| Symbol | `vip`、`calculate` | `Value::Symbol(String)` | 作为 portable value 时受规则限制 |
| List | `(list 1 2 3)` | `Value::List(Vec<Value>)` | 是 |
| Map | `(map "id" "1")` | `Value::Map(BTreeMap<...>)` 的概念 | key 合法时是 |
| Ok / Err | `(ok value)`、`(err issues)` | `Result` 风格的 guest 值 | 按 portable codec 编码 |
| Variant | `(approved 42)` | `Value::Variant { type, variant, fields }` | 是，带 `$type/$variant/fields`；v4 字段有静态类型 |
| Closure | `(fn (x) (+ x 1))` | 受检查 arena 中的闭包 | 否 |
| Primitive | `+`、`validate` | 可信宿主操作 | 否 |

真实实现见 [Value](/source/rust/crates/yanshu-runtime/src/value.rs.txt)；语法字面量见 [AST](/source/rust/crates/yanshu-syntax/src/ast.rs.txt)。

## Int 是任意精度整数

```lisp
(+ 9223372036854775808 1)
```

结果是 `9223372036854775809`，不会溢出成负数。Rust 宿主使用 `num_bigint::BigInt` 保存 `Int`。

::: warning 不能静默收窄
把 `Value::Int(BigInt)` 改成 `i64` 会破坏现有语言语义。除非未来通过版本化语言变更明确引入范围限制，否则实现、FFI 和 Library Backend 都必须保留任意精度整数。
:::

## Truthiness

只有 `#f` 是假；Nil、空 List、整数 `0` 和空字符串都是真。

```lisp
(if 0 "still true" "never")
```

这与 Go / Rust 的条件类型规则不同。审查条件分支时，不要把 `0`、`""` 或空集合自动当成 false。

## List 与 Map 不原地修改

```lisp
(list "a" "b")
(first (list 10 20))
(rest (list 10 20))

(let ((before (map "completed" #f))
      (after (assoc before "completed" #t)))
  (list before after))
```

`assoc` 返回新 Map；`before` 仍保持原值。Map key 必须满足 portable value 和 JSON 边界要求，Web 数据通常使用 String key。

常用集合操作：

- List v1：`list`、`empty?`、`length`、`first`、`rest`；
- List v2：`list-map`、`list-filter`、`list-fold`、`sum`；每访问一个元素都额外消耗 fuel，回调继续按正常表达式计费；
- `list@1` Library：`reverse`、`append`、`contains?`，以及返回 Result 的 `get`、`take`、`drop`、`slice`；
- `map@1` Library：`size`、`keys`、`values`、`entries`、`contains-value?`、可恢复 `remove`，以及显式冲突策略的三种 `merge-*`。
- Map：`map`、`get`、`get-or`、`has-key?`、`assoc`；
- 类型判断：`integer?`、`boolean?`、`string?`、`list?`、`map?`。

## Result 是数据，不是宿主异常

```lisp
(let ((checked (validate task-create body)))
  (if (ok? checked)
      (api-response 201 (result-value checked))
      (api-error 400 "VALIDATION_FAILED" "invalid body"
                 (result-value checked))))
```

`Ok` / `Err` 用于预期内的业务分支。类型错误、fuel 耗尽或调用深度超限属于解释器诊断，两者不要混用：

| 情况 | 表达方式 | Web 结果 |
| --- | --- | --- |
| 用户字段不合法 | `Err(issues)` | handler 可返回 400 |
| 资源不存在 | `api-error 404 ...` | 稳定业务错误 |
| primitive 参数类型错误 | 解释器 diagnostic | 宿主转成不泄漏细节的 500 |
| fuel 耗尽 | 解释器 diagnostic | 请求失败且事务丢弃 |

v2 的 `checked-quotient` / `checked-remainder` 把除零转换成带稳定 `DIVIDE_BY_ZERO` code 的 `Err`，业务可以检查后回退。普通 `quotient`、类型错误、fuel 耗尽、能力越权和宿主失败仍然 fail-loud；语言没有一个能把这些系统诊断全部吞掉的 `try/catch`。

## v3 Variant 是封闭数据

`data` 声明的构造器产生 Variant，而不是无标签 List。Bundle 链接后，一个值的类型名和 variant 名都带模块命名空间：

```json
{
  "$type": "policy/decision",
  "$variant": "policy/approved",
  "fields": [42]
}
```

因此两个模块都声明 `approved` 也不会在运行值里混淆。构造器本身和 Closure 一样不可序列化；调用构造器得到的 Variant 可以安全输出，并可由 `match` 解构。

v4 还会在 export 边界递归检查 Variant、List 和 Result。静态 `any` 允许 JSON 动态 lookup，但不允许实际返回值绕过 signature。

## Nil 与 JSON null 的当前边界

JSON `null` 和空数组 `[]` 进入 guest 后都会映射为 Nil，Nil 输出 JSON 时统一编码为 `[]`。因此当前 round-trip **不会保留** `null` 与 `[]` 的区别。语言还没有独立的 `Null` 值，业务 API 更适合使用明确 Schema、optional 字段和布尔默认值，不要用 `null` / 空列表差异承载业务含义。

## Portable value

跨 CLI、HTTP、版本测试和 Library Backend 的值必须可序列化：不能包含 Closure 或 Primitive，集合还必须满足节点数、深度和字符串长度上限。这个边界防止可信宿主对象被 guest 反射或越权携带。

继续阅读 [Schema 与统一错误](/backend/schema-errors) 或 [标准库与 Library Backend](/language/standard-library)。
