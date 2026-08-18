# Schema 与统一错误

AI-Evolve 把“请求 body 是不是合法业务数据”从 handler 的手写分支移入 Parser 持有的 Schema。它类似 Go validator 的声明，但会先编译成语言自己的 Schema AST；也类似 Rust 中用一个受限 schema enum 驱动校验。

## 声明一个对象 Schema

```lisp
(schema task-create
  (object
    (required "id" (string 1 64))
    (required "title" (string 1 120))
    (optional "completed" boolean #f)))
```

含义：

- `id` 必须存在，且是长度 1～64 的字符串；
- `title` 必须存在，且是长度 1～120 的字符串；
- `completed` 可以省略，但出现时必须是布尔；
- 省略 `completed` 时，成功结果会补上 `#f`；
- 未声明字段一律拒绝。

Schema 名称在程序环境中是编译器持有的特殊值，不是可调用函数，也不能与定义或 primitive 重名。

## 支持的 Schema

```text
any
string
integer
boolean
(string MIN-LENGTH MAX-LENGTH)
(integer MINIMUM MAXIMUM)
(list ITEM-SCHEMA MIN-LENGTH MAX-LENGTH)
(object FIELD ...)

FIELD = (required "name" SCHEMA)
      | (optional "name" SCHEMA)
      | (optional "name" SCHEMA DEFAULT)
```

Parser 限制 Schema 数量、嵌套深度、字段数和集合最大长度；默认值在启动前就会验证。当前不支持 Schema 引用与递归。

真实实现：[Parser 中的 Schema 语法](/source/rust/crates/ail-syntax/src/parser.rs.txt)、[校验器](/source/rust/crates/ail-runtime/src/schema.rs.txt)、[Schema AST](/source/rust/crates/ail-syntax/src/ast.rs.txt)。

## `validate` 返回 Result

```lisp
(let ((validated (validate task-create (get request "body"))))
  (if (ok? validated)
      (api-response 201 (result-value validated))
      (api-error 400
                 "VALIDATION_FAILED"
                 "request body failed schema validation"
                 (result-value validated))))
```

校验失败是普通 `Err` 数据，不是解释器异常。这个区分非常重要：用户少传字段应得到 400，而不是内部 500。

每访问一个 Schema 节点都会消耗解释器 fuel，最多返回 32 个 issue，避免恶意 body 生成无限错误列表。

## Issue 格式

```json
{
  "path": "/title",
  "code": "SCHEMA_REQUIRED",
  "message": "required field is missing"
}
```

`path` 使用 JSON Pointer；嵌套列表可能出现 `/items/0/name`。常见稳定 code：

| code | 含义 |
| --- | --- |
| `SCHEMA_TYPE` | 值类型错误 |
| `SCHEMA_REQUIRED` | 缺少必填字段 |
| `SCHEMA_MIN_LENGTH` / `SCHEMA_MAX_LENGTH` | 字符串或列表长度越界 |
| `SCHEMA_MINIMUM` / `SCHEMA_MAXIMUM` | 整数范围越界 |
| `SCHEMA_ADDITIONAL_PROPERTY` | 对象含未声明字段 |
| `SCHEMA_ISSUES_TRUNCATED` | 超过 issue 上限，其余被省略 |

客户端应该以 `code` 和 `path` 做程序判断，把 `message` 当作公共可读说明。

## 统一 API 错误信封

```lisp
(api-error 400
           "VALIDATION_FAILED"
           "request body failed schema validation"
           issues)
```

产生：

```json
{
  "error": {
    "code": "VALIDATION_FAILED",
    "message": "request body failed schema validation",
    "details": []
  }
}
```

`code` 必须是有界的大写标识符，例如 `TASK_NOT_FOUND`；message 也有长度上限。路由错误、协议错误和内部错误都使用同一层 `error.code/message/details`，客户端不必支持第二种错误形状。

## 内部错误不会泄漏实现

客体解释器诊断可能包含函数名、key 或内部细节。service 边界不会直接把它发给 HTTP 客户端，而是返回：

```json
{
  "error": {
    "code": "INTERNAL_ERROR",
    "message": "request could not be completed",
    "details": {"requestId": "req-..."}
  }
}
```

完整诊断只进入宿主 observation。这样既能让开发者/AI 定位问题，又不会把内部信息变成公开 API。

## 一个失败请求怎样穿过系统

请求：

```json
{"id":"task-1","title":"","owner":"unexpected"}
```

结果同时包含：

- `/title` → `SCHEMA_MIN_LENGTH`；
- `/owner` → `SCHEMA_ADDITIONAL_PROPERTY`。

handler 选择 400 `VALIDATION_FAILED`，事务不做任何写入。对应回归场景位于 [scenarios.json](/source/examples/tasks/scenarios.json.txt)，候选版本必须继续通过这些场景才能晋升。
