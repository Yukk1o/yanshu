# 能力与副作用

衍术将“程序想做什么”和“宿主允许做什么”分开。程序在源码中声明 capability，宿主在运行时注入实现；两边缺少任意一边都会失败。

## 声明需要的能力

```lisp
(program
  (name audit-example)
  (version 4)
  (capabilities log)

  (signature audit (fn (string) string))
  (def audit
    (fn (message)
      (do
        (log (map "message" message))
        message)))

  (export audit))
```

`(capabilities log)` 是这份程序的权限上限，不是自动授权。CLI 为 `log` 提供一个有界 adapter；如果宿主没有提供它，调用会返回稳定诊断，不会静默忽略。

## 当前可用能力

| capability | 操作 | 用途 |
| --- | --- | --- |
| `log` | `log` | 把有界、可移植值交给宿主日志器 |
| `clock` | `now-ms` | 读取宿主提供的 Unix 毫秒时间 |
| `kv` | `kv-get`、`kv-put`、`kv-delete`、`kv-list` | 在当前请求事务中读写键值数据 |

声明 `kv` 不会把数据库连接串或文件路径暴露给 `.yan`。程序只看到固定的 portable API。

## 调用顺序与事务

```lisp
(do
  (kv-put key value)
  (log (map "event" "saved" "key" key))
  (api-response 201 value))
```

`do` 让副作用的顺序可见。Web handler 正常返回合法 response 时，KV 事务才提交；后续表达式产生诊断或返回非法 response 时，本次请求的写入丢弃。

capability 调用和宿主返回值同样受 fuel、深度、节点数和字节数上限约束。

## 静态计算 capability 闭包

v4 分析器从导出函数出发，穿过普通调用、递归、模块 import 和已知高阶回调，计算真正可达的能力。

```powershell
.\yanshu.exe check policy.yan
```

输出中会包含类似：

```json
{
  "capabilityClosure": ["log"],
  "declaredCapabilities": ["log"],
  "unusedCapabilities": []
}
```

- 调用到但没声明的能力会使分析失败；
- 声明但没有任何导出路径需要的能力会进入 `unusedCapabilities`；
- 分析器无法解析高阶函数参数的效果时会失败关闭，不会猜它是纯函数。

## 在只读审查视图中识别副作用

```powershell
.\yanshu.exe review policy.yan --text
```

审查投影使用 `log!(...)` 标记直接 capability 调用，使用 `audit!(...)` 标记会传递到 capability 的普通函数。`!` 只是人类审查提示，不是 `.yan` 语法，审查文本也不能执行或回写。

## Library 不是 capability

```lisp
(libraries (text 1))
```

Library 用于纯文本等确定性计算，capability 用于观测或修改宿主状态。两者都有版本/契约和计费边界，但只有 capability 会进入效果闭包。

## 常见错误

### 声明了 capability 但宿主不提供

源码通过 Parser 不意味着任意命令都有对应 adapter。例如单文件 CLI 不会为普通执行自动提供 `kv` 或 `clock`；Web service 宿主才会在请求事务中提供它们。

### 把密钥作为普通参数传入 guest

这会绕过 capability 设计的隔离意图。新的外部集成应在宿主侧定义窄接口，不应把 token、文件句柄或数据库客户端放进 portable value。

下一步阅读[类型、效果与只读审查](/language/types-effects-review)。
