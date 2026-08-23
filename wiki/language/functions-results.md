# 函数、控制流与 Result

衍术程序主要由小函数组成。值不可变，`let` 创建局部名字，`if` / `cond` 选择分支，`Result` 表示可预期的业务成功或失败。

## 定义与导出函数

```lisp
(signature add-tax (fn (integer integer) integer))
(def add-tax
  (fn (amount rate)
    (+ amount (quotient (* amount rate) 100))))
(export add-tax)
```

- `def` 将名字绑定到值；
- `fn` 创建词法闭包，参数从左到右绑定；
- v4 的每个导出 `def` 都必须有同名 `signature`；
- 只有 `export` 中的名字能被 CLI、route 或其他模块调用。

闭包可以使用创建时作用域中的值：

```lisp
(let ((offset 10))
  (fn (value) (+ value offset)))
```

递归通过顶层 `def` 实现，并受 fuel 和调用深度限制。

## 用 `let` 命名中间结果

```lisp
(let ((subtotal (+ price shipping))
      (discount (quotient subtotal 10))
      (total (- subtotal discount)))
  total)
```

binding 从上到下求值，后一个 binding 可以引用前面的名字。名字只在 `let` body 中可见，没有赋值 form。

::: tip 可读性
当一个表达式同时在做解构、计算和决策时，先用 `let` 给业务中间量命名。这比让人类或 AI 重复解析多层括号更容易审查。
:::

## `if`、`cond`、`and` 与 `or`

`if` 始终需要条件、成立分支和不成立分支：

```lisp
(if (< amount 0) "invalid" "accepted")
```

多个业务分支使用 `cond`：

```lisp
(cond
  ((< amount 0) "invalid")
  ((>= amount 1000) "manual-review")
  (else "approved"))
```

`cond` 必须以 `(else expression)` 结尾。`and` 与 `or` 从左到右短路，被跳过的表达式不会求值：

```lisp
(and (>= amount 0) (< amount limit))
(or (= role "finance") (= role "admin"))
```

它们返回实际选中的操作数，不一定返回 Bool。只有 `#f` 是假；`0`、`""`、Nil 和空 List 都是真。

## 集合处理

常用 List 函数：

```lisp
(list-map (fn (amount) (* amount 2)) amounts)
(list-filter (fn (amount) (> amount 0)) amounts)
(list-fold (fn (total amount) (+ total amount)) 0 amounts)
(sum amounts)
```

`list-map`、`list-filter` 和 `list-fold` 会对元素按顺序调用回调。遍历本身和回调内的表达式都会计入 fuel，因此不是隐藏的免费循环。

Map 不会原地修改：

```lisp
(let ((before (map "status" "draft"))
      (after (assoc before "status" "submitted")))
  (list before after))
```

## 用 Result 表示可恢复失败

```lisp
(let ((division (checked-quotient total count)))
  (if (ok? division)
      (ok (result-value division))
      (err (map "code" "EMPTY_BATCH"
                "message" "cannot average an empty batch"))))
```

`checked-quotient` 在除数为零时返回 `Err`，调用者可以决定降级策略。常用 Result 函数：

| 函数 | 用途 |
| --- | --- |
| `(ok value)` / `(err value)` | 构造成功或失败值 |
| `(ok? result)` / `(err? result)` | 判断分支 |
| `(result-value result)` | 取出 `Ok` 或 `Err` 携带的值 |
| `(unwrap result)` | 只适合“Err 就是程序缺陷”的位置 |

Result 不是通用异常捕获。类型错误、fuel 耗尽、调用深度超限、capability 越权和宿主失败仍会 fail-loud，业务程序不能把它们全部吞掉。

## 用 `do` 表达明确顺序

```lisp
(do
  (kv-put key expense)
  (log (map "event" "expense-created" "id" id))
  (api-response 201 expense))
```

`do` 从左到右执行，返回最后一个表达式。它只表达顺序，不会自动授权；上例仍需要声明并注入 `kv` 和 `log`。

## 常见错误

### 导出函数缺少签名

v4 的 `(export decide)` 必须对应 `(signature decide ...)`。签名的参数数量、函数 body 和返回值类型必须一致。

### 把空字符串当成 false

```lisp
(if "" "this branch runs" "not reached")
```

需要判断空字符串时，请使用明确比较或 Schema 长度约束。

### 用普通 `quotient` 实现业务降级

`quotient` 除零会中止本次执行。当除零是可预期输入时，使用 `checked-quotient` 并处理 Result。

下一步学习[模块、用户数据类型与 Bundle](/language/modules-bundles)。
