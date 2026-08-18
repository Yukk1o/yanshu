# 语言语法入门

这页不假设你会 Lisp。先掌握一个阅读规则：**左括号后第一个词通常是操作，后面都是参数。**

```lisp
(+ 1 2)              ; 相当于 1 + 2
(string-append a b)  ; 相当于 a + b / format!("{a}{b}")
(get user "name")   ; 相当于 user["name"]
```

`.ail` 源码只有数据和函数调用，没有 Racket 宏，也不能嵌入 Racket 代码。

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

逐段看：

### `(program ...)`

整个文件必须只有这一个顶层 S 表达式。类似 Go 的一个 package 文件，或 Rust 中被编译成 `Program` IR 的一个模块。

### `(name discount)`

程序的逻辑名称。它是符号 `discount`，不是字符串。AI 修复候选时通常应保持名字不变。

### `(version 1)`

这是客体语言程序自己声明的正整数版本，不是部署版本。真正部署时，源码还会计算 SHA-256，内容不同就得到不同的制品 ID。

### `(capabilities)`

空列表表示纯程序不需要副作用。如果写成 `(capabilities kv clock log)`，宿主必须提供对应能力，否则执行失败。

可以把它理解成 Rust 函数签名中显式传入 trait，而不是使用隐藏的全局变量。

### `(def calculate-discount ...)`

定义一个顶层名称。右边是 `(fn ...)` 闭包：

```lisp
(fn (price user-type) BODY)
```

对应 Go：

```go
func calculateDiscount(price *big.Int, userType string) *big.Int { /* BODY */ }
```

对应 Rust：

```rust
fn calculate_discount(price: BigInt, user_type: &str) -> BigInt { /* BODY */ }
```

区别是 `.ail` 当前没有静态类型声明；类型错误由受限解释器产生稳定诊断。

### `(if CONDITION THEN ELSE)`

三个分支都必须写。只有 `#f` 是假，空列表、整数 `0` 和空字符串都是真。

```lisp
(if (= user-type "vip")
    (- price (quotient price 10))
    price)
```

这里先计算 `user-type == "vip"`。为真时返回 `price - price/10`，否则原价返回。`quotient` 是整数除法。

### `(export calculate-discount)`

只有导出的定义能被 CLI 或路由调用。类似 Go 的公开标识符，或 Rust 的 `pub fn`；但这里采用显式白名单。

真实源码：[discount/v2.ail](/source/examples/discount/v2.ail.txt)，测试：[discount/tests.json](/source/examples/discount/tests.json.txt)。

## 绑定局部变量：`let`

```lisp
(let ((id (get body "id"))
      (key (string-append "task/" id))
      (existing (kv-get key #f)))
  (if existing
      (api-error 409 "TASK_EXISTS" "task id already exists")
      (api-response 201 body)))
```

`let` 的绑定从上到下求值，后面的绑定能引用前面已经绑定的名称。Go 类比：

```go
id := body["id"]
key := "task/" + id
existing := tx.GetOr(key, false)
if existing != false { /* ... */ }
```

这些绑定是词法作用域内的名字，不是可变变量；没有 `set!`。

## 顺序执行副作用：`do`

纯函数通常只返回一个表达式。需要先写 KV、再记日志、最后返回时使用 `do`：

```lisp
(do
  (kv-put key task)
  (log (map "event" "task-created" "id" id))
  (api-response 201 task))
```

表达式从左到右执行，整个 `do` 的值是最后一个表达式的值。类似 Go / Rust 的普通代码块，但副作用函数仍受 capability 限制。

## 数据类型

| `.ail` 值 | 示例 | Go / Rust 近似概念 |
| --- | --- | --- |
| Nil | `'()` 或 JSON `null` 转入后 | `nil` / `Option::None`，但也表示空 List |
| Bool | `#t`、`#f` | `bool` |
| Int | `42`、`-3`，也可超过 64 位 | Go `big.Int` / Rust `num_bigint::BigInt` |
| String | `"task-1"` | `string` / `String` |
| Symbol | `vip`、`calculate` | interned identifier / enum-like atom |
| List | `(list 1 2 3)` | `[]Value` / `Vec<Value>` |
| Map | `(map "id" id "title" title)` | `map[string]Value` / `HashMap<String, Value>` |
| Ok / Err | `(ok value)`、`(err issues)` | `(value, nil)` / `Result<T, E>` |
| Closure | `(fn (x) (+ x 1))` | closure |
| Schema | 顶层 `(schema ...)` 绑定 | 编译器持有的校验器描述 |

::: warning Nil 的当前语义
JSON `null` 进入客体后会变成空列表值，因此 v0.3 还没有把 `Null` 与 `List()` 完全分离。写业务时更推荐明确字段 Schema 和布尔默认值。
:::

## List 与 Map

```lisp
(list "a" "b")
(empty? (list))
(length (list 10 20))
(first (list 10 20))
(rest (list 10 20))
```

Map 用交替的 key/value 创建：

```lisp
(map "id" "task-1" "completed" #f)
(get task "id")
(get-or task "owner" "nobody")
(has-key? task "completed")
(assoc task "completed" #t)
```

`assoc` 返回新 Map，不原地修改旧 Map。这与 Rust 中消费/克隆后返回新值、或函数式 persistent map 的思路更接近。

## Result：业务失败不是解释器崩溃

Schema `validate` 返回 `Ok` 或 `Err`：

```lisp
(let ((validated (validate task-create body)))
  (if (ok? validated)
      (api-response 201 (result-value validated))
      (api-error 400
                 "VALIDATION_FAILED"
                 "request body failed schema validation"
                 (result-value validated))))
```

Rust 近似写法：

```rust
match validate(&TASK_CREATE, body) {
    Ok(normalized) => api_response(201, normalized),
    Err(issues) => api_error(400, "VALIDATION_FAILED", "...", issues),
}
```

`result-value` 在检查分支后提取任一 payload。`unwrap` 遇到 Err 会产生解释器诊断，业务 handler 通常不应对用户输入使用它。

## 纯 primitive 速查

- 算术：`+`、`-`、`*`、`quotient`、`remainder`；
- 比较：`=`、`<`、`<=`、`>`、`>=`、`not`；
- 类型判断：`integer?`、`boolean?`、`string?`、`list?`、`map?`；
- 集合：`list`、`empty?`、`length`、`first`、`rest`、`map`、`get`、`get-or`、`has-key?`、`assoc`；
- 字符串：`string-append`；
- 结果：`ok`、`err`、`ok?`、`err?`、`result-value`、`unwrap`；
- Web：`validate`、`api-response`、`api-error`。

解释器的真实安装列表在 [runtime.rkt](/source/src/runtime.rkt.txt)，LLM 收到的语言说明在 [evolver.rkt](/source/src/evolver.rkt.txt)。

## 有副作用的 capability

| 声明 | 得到的函数 | 权限 |
| --- | --- | --- |
| `log` | `log` | 把结构化值交给宿主 logger |
| `clock` | `now-ms` | 读取宿主提供的 Unix 毫秒时间 |
| `kv` | `kv-get`、`kv-put`、`kv-delete`、`kv-list` | 在当前请求事务内读写 KV |

没有声明就没有绑定；声明了但宿主没有提供，也会失败。这是 capability security 的核心，不是普通框架中“随处可 import 数据库包”的方式。

## 不支持的东西

当前没有宏、异常捕获、可变变量、并发、模块导入、递归 Schema、浮点数、日期类型、用户自定义类型或任意宿主调用。不要把 Racket 语法误认为 `.ail` 一定支持；以 [Parser](/source/src/parser.rkt.txt) 和 [语言规格](/source/docs/spec-v0.1.md.txt) 为准。

::: warning 整数不能在迁移时静默收窄
当前 `Int` 继承 Racket `exact-integer` 的任意精度语义。Rust 宿主需要使用
`BigInt`；改成 `i64` 会破坏现有程序，除非未来把它作为版本化语言变更正式引入。
:::

接下来用一个真实请求学习[Web 路由](/backend/web)。
