# 语言语法入门

这页不假设你会 Lisp。先掌握一个阅读规则：**左括号后第一个词通常是操作，后面都是参数。**

```lisp
(+ 1 2)              ; 相当于 1 + 2
(string-append a b)  ; 相当于 a + b / format!("{a}{b}")
(get user "name")   ; 相当于 user["name"]
```

`.ail` 只接受语言规格定义的数据、声明和表达式。未知 form 会在解析阶段拒绝，不能嵌入任意宿主代码。

## 从一个完整程序开始

```lisp
(program
  (name discount)
  (version 1)
  (capabilities)

  (def calculate-discount
    (fn (price user-type)
      (if (= user-type "vip")
          (- price (quotient price 10))
          price)))

  (export calculate-discount))
```

### `(program ...)`

每个文件必须只有一个顶层 `program`。Parser 会把它转换为结构化 `Program`，而不是把文本交给通用求值器。

### `(name discount)`

程序的逻辑名称。`discount` 是 Symbol，不是 String。名称和定义必须唯一。

### `(version 1)`

语言内声明的语义版本。当前只接受 `1` 和 `2`：未知版本会以 `PROGRAM_UNSUPPORTED_VERSION` 拒绝；v1 源码使用 v2 form 会得到带 `feature / actualVersion / minimumVersion` 的 `PROGRAM_FEATURE_REQUIRES_VERSION`。

它与部署制品的 SHA-256 不同：`version` 决定“按哪套语言规则解释”，源码 hash 决定“究竟是哪一份不可变代码”。相同语言版本的两个源码内容仍会得到不同制品 ID。

### `(capabilities)`

空列表表示不请求副作用。Web 程序可以写：

```lisp
(capabilities kv clock log)
```

声明类似 Rust trait bound 或 Go 窄接口，但宿主还必须显式注入实现；源码不能凭声明获得文件、网络或密钥。

### `(def NAME EXPR)`

定义顶层绑定。右侧可以是值或闭包：

```lisp
(def calculate-discount
  (fn (price user-type) BODY))
```

可以用 Rust 心智模型理解为：

```rust
fn calculate_discount(price: BigInt, user_type: &str) -> BigInt {
    /* BODY */
}
```

`.ail` 当前没有静态类型标注；类型错误由受限解释器产生稳定诊断。

### `(export NAME)`

只有显式导出的定义能被 CLI 或 route 调用。它像 `pub fn` 白名单，而不是按命名约定自动公开。

真实程序见 [discount/v2.ail](/source/examples/discount/v2.ail.txt)。

## 字面量与 quote

源码原子包括任意精度整数、布尔、字符串和 Symbol：

```lisp
42
-3
#t
#f
"hello"
vip
```

单引号是 `(quote ...)` 的简写，使 datum 作为数据而不是调用求值：

```lisp
'vip
'(a b c)
(quote (a b c))
```

值的可移植性、JSON 编码与 Nil 边界见[数据模型](/language/data-model)。

## 条件：`if`

```lisp
(if CONDITION THEN ELSE)
```

三个位置都必须存在。只有 `#f` 是假；空列表、整数 `0` 和空字符串都是真。

```lisp
(if (= user-type "vip")
    (- price (quotient price 10))
    price)
```

`quotient` 是整数除法，除数为零会产生解释器诊断。

## v2 业务条件：`and`、`or`、`cond`

```lisp
(and (> total 0) (< total 10000))
(or (= action "reject") (> monthly-total 20000))

(cond
  ((= action "reject") "rejected")
  ((> total 10000) "manual-review")
  (else "approved"))
```

`and` / `or` 从左到右短路，并返回实际选中的操作数；空的 `(and)` 是 `#t`，空的 `(or)` 是 `#f`。它们是特殊 form，不是普通函数，因此被短路的表达式绝不会求值。

`cond` 必须以显式 `(else expression)` 结尾，且 `else` 只能出现在最后。这个限制让业务分支是穷尽的，也让 LLM 与审查者不必猜“没有命中时返回什么”。三者只在 `(version 2)` 可用。

## 局部绑定：`let`

```lisp
(let ((id (get body "id"))
      (key (string-append "task/" id))
      (existing (kv-get key #f)))
  (if existing
      (api-error 409 "TASK_EXISTS" "task id already exists")
      (api-response 201 body)))
```

绑定从上到下求值，后面的绑定可以引用前面已经绑定的名字。对应 Rust 风格：

```rust
let id = body.get("id");
let key = format!("task/{id}");
let existing = tx.get_or(&key, false);
if existing { /* ... */ }
```

名字在词法作用域内绑定一次；当前没有赋值 form。

## 函数与词法闭包：`fn`

```lisp
(fn (x y) (+ x y))
```

闭包捕获创建时可见的词法环境：

```lisp
(let ((offset 10))
  (fn (value) (+ value offset)))
```

参数个数在调用时检查。递归通过顶层定义实现，并受 fuel 与调用深度限制。

## 顺序表达式：`do`

`do` 从左到右执行，结果是最后一个表达式：

```lisp
(do
  (kv-put key task)
  (log (map "event" "task-created" "id" id))
  (api-response 201 task))
```

它提供受控的效果顺序，但不会绕过 capability。请求只有在 handler 正常完成并返回合法 response 后才提交 KV 事务。

## 普通函数调用

除特殊 form 外，非空 List 都按函数调用处理：

```lisp
(FUNCTION ARGUMENT ...)
```

先求值函数位置，再从左到右求值参数。可调用值只能是 guest Closure 或宿主安装的可信 Primitive。

## 顶层 Web 声明

Web 程序还可以声明 Schema 和 route：

```lisp
(schema task-create
  (object
    (required "id" (string 1 64))
    (required "title" (string 1 120))))

(route POST "/tasks" create-task)
```

Schema 名称不是普通函数；route handler 必须同时有 `def` 和 `export`。详见 [Schema](/backend/schema-errors) 与 [Web DSL](/backend/web)。

## 纯 Primitive 速查

- 算术：`+`、`-`、`*`、`quotient`、`remainder`；v2 另有返回 Result 的 `checked-quotient`、`checked-remainder`；
- 比较：`=`、`<`、`<=`、`>`、`>=`、`not`；
- 类型判断：`integer?`、`boolean?`、`string?`、`list?`、`map?`；
- List：`list`、`empty?`、`length`、`first`、`rest`；v2 另有 `list-map`、`list-filter`、`list-fold`、`sum`；
- Map：`map`、`get`、`get-or`、`has-key?`、`assoc`；
- String：`string-append`；v2 另有 `number->string`；
- Result：`ok`、`err`、`ok?`、`err?`、`result-value`、`unwrap`；
- Web：`validate`、`api-response`、`api-error`；v2 另有带 fuel 成本的 `validate-report`。

真实安装表见 [ail-runtime](/source/rust/crates/ail-runtime/src/lib.rs.txt)。

## Capability Primitive

| 声明 | 可见函数 | 权限 |
| --- | --- | --- |
| `log` | `log` | 把结构化值交给宿主 logger |
| `clock` | `now-ms` | 读取宿主提供的 Unix 毫秒时间 |
| `kv` | `kv-get`、`kv-put`、`kv-delete`、`kv-list` | 在当前请求事务内读写 KV |

没有声明就没有绑定；声明了但宿主未提供也会失败。安全语义见[能力边界](/evolution/security)。

## 版本化 Library

```lisp
(libraries (text 1))

(text/length "AI语言")
(text/starts-with? "AI language" "AI")
```

程序选择 `text@1` 契约，不能指定具体 crate 或 backend。详见[标准库与 Library Backend](/language/standard-library)。

## 当前不支持

当前没有宏、通用异常捕获、可变变量、并发、用户模块导入、递归 Schema、浮点数、日期类型、用户自定义类型或任意宿主调用。v2 预期业务失败通过显式 Result 表达，不能捕获 fuel、能力或宿主诊断。以 [v0.6 语言规格](/source/docs/spec-v0.6.md.txt) 和 [Rust Parser](/source/rust/crates/ail-syntax/src/parser.rs.txt) 为准，不要因为语法外观相似就假设其它 form 可用。

::: warning Int 必须保持任意精度
当前 `Int` 语义允许超过 64 位，Rust 实现使用 `num_bigint::BigInt`。除非未来作为版本化语言变更正式引入范围限制，否则不能静默收窄到 `i64`。
:::

接下来用真实请求学习 [Web DSL 与路由](/backend/web)。
