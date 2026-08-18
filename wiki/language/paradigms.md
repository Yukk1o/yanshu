# 语言范式

AI-Evolve 不是“给现有语言加一个 LLM API”。它把 **AST、受限解释、显式能力和版本门禁**组合成一套适合人类与 AI 协作的语言范式。

## 1. 代码即数据

源码使用 S 表达式，因此调用、条件、函数和绑定天然是树：

```lisp
(if (> score 80) "pass" "retry")
```

```text
If
├─ Call ">"
│  ├─ Var score
│  └─ Int 80
├─ String "pass"
└─ String "retry"
```

这给 AI 带来三个直接好处：

1. 生成结果容易被 Parser 验证，未知节点会被拒绝；
2. route、Schema、capability、错误码可以从 AST 提取，不必猜文本；
3. 未来可以在 AST 层做局部 patch、结构化 diff 和只读审查视图。

“代码即数据”不意味着可以随意执行生成结果。AST 只是候选制品，必须先通过验证门禁。

## 2. 函数式默认

语言提供词法闭包、`let`、`if`、`do` 和不可变集合，没有 `set!` 或共享可变全局状态。

```lisp
(let ((subtotal (+ price shipping))
      (discount (quotient subtotal 10)))
  (- subtotal discount))
```

每个名字在词法作用域内绑定一次。`assoc` 返回新 Map，不原地改旧值；业务失败使用显式 `Ok` / `Err`。这让候选行为更适合：

- 用输入/输出案例重放；
- 比较新旧版本；
- 缩小隐式状态导致的测试盲区；
- 把副作用集中到清晰的 capability 调用。

`do` 允许按顺序表达副作用，但它不会把权限变成隐式全局变量：

```lisp
(do
  (kv-put key value)
  (log (map "event" "saved" "key" key))
  (api-response 201 value))
```

## 3. 受限解释，而非任意求值

宿主只遍历语言定义的 AST 节点。执行同时受以下预算控制：

- 源码节点数和嵌套深度；
- evaluation fuel；
- 函数调用深度；
- Schema 节点、字段、集合和 issue 数量；
- HTTP 请求与响应边界。

未知 form、类型错误、除零、fuel 耗尽都会成为带稳定 `code/message/details` 的诊断，不会退化为随宿主变化的堆栈文本。

## 4. Capability security

程序必须在顶层声明需要的能力：

```lisp
(capabilities kv clock log)
```

声明只是请求，不是授权。宿主还必须显式注入实现；缺少任一侧都会失败。当前能力只有：

| capability | 可见操作 | 不会因此获得 |
| --- | --- | --- |
| `kv` | `kv-get`、`kv-put`、`kv-delete`、`kv-list` | 文件句柄、数据库连接串 |
| `clock` | `now-ms` | 系统命令、任意时间 API |
| `log` | `log` | 日志文件路径、观测后端凭据 |

guest 默认看不到文件、socket、环境变量和 provider API key。未来新增邮件、队列或网络能力时，也应先定义窄契约、预算和审计语义。

## 5. 契约进入语言

传统框架常把路由、DTO、错误码、权限和业务实现散在不同库中。AI-Evolve 把关键部分放入同一棵 Program AST：

```lisp
(schema task-create ...)
(route POST "/tasks" create-task)
(capabilities kv clock)
(def create-task ...)
(export create-task)
```

因此审查者可以先看“系统允许什么”，再看“handler 怎样实现”：

- Schema diff 是输入面变化；
- route diff 是公开 API 变化；
- capability diff 是权限变化；
- `api-error` diff 是客户端契约变化；
- 测试场景 diff 是业务意图变化。

## 6. 演化是制品流，不是自修改

运行期演化不等于原地改写正在执行的闭包。AI-Evolve 使用分阶段制品流：

```text
active source → LLM candidate → parse → full suite
                                      │ pass
                                      ▼
                              immutable version
                                      │ explicit promote
                                      ▼
                                active pointer
```

每个版本以源码哈希标识；每个请求固定一次活动版本。候选失败不会污染 active，已开始的请求也不会执行到一半切换代码。

接下来读[语法入门](/language/syntax)、[安全模型](/evolution/security)和[演化生命周期](/evolution/lifecycle)。
