# 标准库

Yanshu 的标准库是纯函数、确定性且有 fuel 计量的 API。程序必须声明精确版本，源码才能使用对应函数：

```lisp
(program
  (name text-demo)
  (version 4)
  (capabilities)
  (libraries (text 2))

  (signature normalize (fn (string) map))
  (def normalize
    (fn (value)
      (let ((trimmed (text/trim value))
            (parts (text/split trimmed ",")))
        (map "text" (text/lowercase trimmed)
             "parts" parts
             "joined" (text/join parts " / ")))))

  (export normalize))
```

`(libraries (text 2))` 选择 `text@2`。未知版本、重复声明或由用户定义占用 `text/...` 命名空间都会在加载时失败。

## text@2

`text@2` 包含 `text@1` 的全部函数，并增加常用的文本整理与集合转换：

| 函数 | 参数 | 结果 |
| --- | --- | --- |
| `text/length` | String | Unicode scalar 数量的 Int |
| `text/starts-with?` | String, String | Bool |
| `text/ends-with?` | String, String | Bool |
| `text/contains?` | String, String | Bool |
| `text/replace` | String, String, String | String |
| `text/trim` | String | 去掉两端 Unicode 空白后的 String |
| `text/lowercase` | String | Unicode 小写 String |
| `text/uppercase` | String | Unicode 大写 String |
| `text/split` | String, String | List&lt;String&gt; |
| `text/join` | List&lt;String&gt;, String | String |
| `text/substring` | String, Int, Int | String |

### 拆分与拼接

```lisp
(text/split "审批,,归档," ",")
; => ("审批" "" "归档" "")

(text/join (list "审批" "归档") " -> ")
; => "审批 -> 归档"
```

`text/split` 使用非空的字面分隔符，并保留开头、中间和结尾的空字段。空分隔符返回 `RUNTIME_LIBRARY_ARGUMENT`，不会隐式按字符拆分。

### Unicode 大小写

```lisp
(text/uppercase "straße")
; => "STRASSE"

(text/lowercase "İSTANBUL")
; => "i̇stanbul"
```

大小写转换使用与区域无关的 Unicode 映射，不读取系统 locale。同一输入在解释器和编译执行中得到相同结果；转换后的字符数可能与输入不同。

### Unicode substring

```lisp
(text/substring "A语言🦀Z" 1 4)
; => "语言🦀"
```

范围采用 `[start, end)`，下标按 Unicode scalar 计算，不按 UTF-8 字节计算。必须满足 `0 ≤ start ≤ end ≤ text/length`，否则返回 `RUNTIME_LIBRARY_ARGUMENT`。

## text@1

`text@1` 仍保持兼容，只提供以下五个函数：

- `text/length`
- `text/starts-with?`
- `text/ends-with?`
- `text/contains?`
- `text/replace`

旧程序不需要迁移；只有需要新 API 时才把声明改为 `(text 2)`。版本是契约边界，`text@1` 程序不能调用 `text/trim` 等 v2 函数。

可运行示例：

- [text@1 示例](/source/examples/libraries/text.yan.txt)
- [text@2 示例](/source/examples/libraries/text-v2.yan.txt)

## 资源与失败边界

标准库调用与普通表达式共享 guest fuel。每个操作的计费模型属于版本化契约；输入越长、输出越大或列表项越多，消耗越高。

文本结果最多 1 MiB。split 结果还受 10,000 个 portable 节点上限约束。后端在分配放大结果前检查上限，失败时返回稳定诊断，而不是继续占用宿主内存。

## Library 与 capability

| | Library | Capability |
| --- | --- | --- |
| 用途 | 纯文本、编码、确定性算法 | KV、clock、log 等外部效果 |
| 声明 | `(libraries (text 2))` | `(capabilities kv clock)` |
| 宿主状态 | 不接触 | 通过窄接口显式接触 |
| 效果闭包 | 不进入 | 进入静态 capability 闭包 |

guest 只能选择可信契约的名称与版本，不能指定 crates.io 包、动态库路径或 provider。宿主如何用 safe Rust、隔离进程或 WASM 实现契约，不会扩大 guest 可见 API。

继续阅读 [语法](/language/syntax)、[能力与副作用](/language/capabilities)和[安全模型](/evolution/security)。
