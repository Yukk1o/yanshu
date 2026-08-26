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

## math@1

`math@1` 面向任意精度整数，所有函数都是确定性纯函数：

| 函数 | 参数 | 结果 |
| --- | --- | --- |
| `math/abs` | Int | 非负绝对值 |
| `math/sign` | Int | `-1`、`0` 或 `1` |
| `math/min` | Int, Int | 较小的 Int |
| `math/max` | Int, Int | 较大的 Int |
| `math/clamp` | Int, Int, Int | 钳制到闭区间的 Int |
| `math/gcd` | Int, Int | 非负最大公约数 |

程序需要显式声明：

```lisp
(libraries (math 1))
```

常见业务用法：

```lisp
(math/clamp 135 0 100)
; => 100

(math/abs -42)
; => 42

(math/sign -42)
; => -1

(math/gcd -42 30)
; => 6
```

`math/clamp` 的参数顺序是 `value, minimum, maximum`，并要求 `minimum <= maximum`。非法区间返回 `RUNTIME_LIBRARY_ARGUMENT`，不会悄悄交换边界。

`math/gcd` 始终返回非负值，`(math/gcd 0 0)` 返回 `0`。它按照两个整数的 magnitude block 乘积计量，因此大整数不会以接近常数的 fuel 运行。

`math@1` 暂不提供 `pow` 和 `lcm`：它们可能显著放大 BigInt，必须先具备可精确预检、预扣 fuel、再分配结果的调用契约，不能把保守误拒绝永久写进 v1。

可运行示例：[math@1 示例](/source/examples/libraries/math.yan.txt)

## digest@1

`digest@1` 用于对文本生成确定性内容摘要：

| 函数 | 参数 | 结果 |
| --- | --- | --- |
| `digest/sha256-text` | String | 64 字符的小写 SHA-256 十六进制 String |
| `digest/sha512-text` | String | 128 字符的小写 SHA-512 十六进制 String |

程序需要显式声明：

```lisp
(libraries (digest 1))
```

函数名中的 `text` 表示输入按 UTF-8 编码后再计算摘要：

```lisp
(digest/sha256-text "abc")
; => "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
```

编码是契约的一部分，同一文本不会受操作系统或 locale 影响。未来即使语言加入 `Bytes`，这两个函数也仍只处理 UTF-8 文本，不会改变旧程序语义。

SHA 摘要不能替代密码哈希、MAC 或数字签名：不要用它保存用户密码，也不要用无密钥摘要判断消息是否来自可信发送者。

可运行示例：[digest@1 示例](/source/examples/libraries/digest.yan.txt)

## json@1

`json@1` 用于 AI、接口 adapter 和业务系统之间交换普通 JSON。解析失败是可恢复的 Result，不会让一条坏数据直接打穿整个 guest 请求：

| 函数 | 参数 | 结果 |
| --- | --- | --- |
| `json/parse` | String | `Result<Any, Map>` |
| `json/stringify-canonical` | Any | `Result<String, Map>` |

```lisp
(libraries (json 1))

(let ((decoded (json/parse "{\"amount\":42,\"tags\":[\"AI\"]}")))
  (if (ok? decoded)
      (result-value decoded)
      (map "status" "invalid-json"
           "issue" (result-value decoded))))
```

类型按直觉映射：JSON `null` 是 Nil，boolean 是 Bool，整数是 Int，string 是 String，array 是 List，object 是只含 String key 的 Map。

### 严格解析

v1 故意不接受小数和指数：

```lisp
(json/parse "1.5")
; => (err (map "code" "JSON_NON_INTEGER_NUMBER" "offset" 1))
```

这样不会暗中引入 IEEE-754 精度损失。需要金额时可以直接使用整数最小货币单位，或使用下面的 `decimal@1` 在外部小数文本与整数系数之间转换。

重复 object key 也会拒绝，包括 escape 后相同的键，例如 `"a"` 与 `"\u0061"`。它返回 `JSON_DUPLICATE_KEY`，不会依赖不同 JSON 库各自的 first-wins 或 last-wins 行为。

### 规范序列化

```lisp
(json/stringify-canonical (map "z" 2 "a" (list #t '())))
; => (ok "{\"a\":[true,null],\"z\":2}")
```

对象键按稳定顺序排列，不输出缩进或无意义空白，整数和 escape 使用固定形式。因此相同 guest 数据总能得到相同文本，适合摘要、缓存键、测试快照和内容寻址。

Symbol、Symbol key Map、Result 与用户 Variant 不是普通 JSON。序列化这些值会返回带 `JSON_UNSUPPORTED_VALUE` 的 Err，不会擅自发明一种以后无法兼容的标签格式。

常见错误码包括：

| code | 含义 |
| --- | --- |
| `JSON_SYNTAX` | JSON 语法或 escape 非法 |
| `JSON_NON_INTEGER_NUMBER` | 出现小数或指数 number |
| `JSON_DUPLICATE_KEY` | object 解码后存在重复键 |
| `JSON_*_LIMIT` | 输入、输出、字符串、节点、深度或整数越界 |
| `JSON_UNSUPPORTED_VALUE` | guest 值不能无损表示为普通 JSON |

错误 Map 不回显原始输入。解析位置 `offset` 使用 UTF-8 byte offset，适合代理和宿主稳定消费。

可运行示例：[json@1 示例](/source/examples/libraries/json.yan.txt)

## decimal@1

`decimal@1` 用“整数系数 + scale”表示精确小数，不使用浮点数。例如系数 `1234`、scale `2` 表示 `12.34`：

| 函数 | 参数 | 结果 |
| --- | --- | --- |
| `decimal/parse-scaled` | String, Int | `Result<Int, Map>` |
| `decimal/format-scaled` | Int, Int | `Result<String, Map>` |
| `decimal/rescale` | Int, Int, Int, String | `Result<Int, Map>` |

### 金额输入与显示

下面的程序把接口文本精确转换为分，并在显示时补足两位小数：

```lisp
(libraries (decimal 1))

(decimal/parse-scaled "12.34" 2)
; => (ok 1234)

(decimal/parse-scaled "-0.5" 2)
; => (ok -50)

(decimal/format-scaled -5 2)
; => (ok "-0.05")
```

解析不接受空白、`+`、指数、千位分隔符或整数部分前导零。小数位少于 scale 时补零；超出的位只有全是 `0` 才能无损忽略：

```lisp
(decimal/parse-scaled "1.2300" 2)
; => (ok 123)

(decimal/parse-scaled "1.234" 2)
; => (err (map "code" "DECIMAL_PRECISION_LOSS" "offset" 4))
```

### 舍入必须写出来

把一个系数从旧 scale 转到新 scale 时，第四个参数必须显式选择舍入模式：

```lisp
(decimal/rescale 125 2 1 "half-up")
; => (ok 13)

(decimal/rescale 125 2 1 "half-even")
; => (ok 12)

(decimal/rescale 125 2 1 "exact")
; => (err (map "code" "DECIMAL_ROUNDING_REQUIRED"))
```

可用模式如下：

| 模式 | 行为 |
| --- | --- |
| `exact` | 不能整除就返回错误 |
| `toward-zero` | 向零截断 |
| `floor` | 向负无穷 |
| `ceiling` | 向正无穷 |
| `half-up` | 最近值，恰好一半时远离零 |
| `half-even` | 最近值，恰好一半时选择偶数系数 |

未知模式不会退回操作系统、数据库或 Rust 的默认规则，而是返回 `DECIMAL_INVALID_ROUNDING_MODE`。这使费用审批、税额和汇率规则在解释器、编译 VM 与不同宿主上保持一致。

scale 必须在 `0..=1024`。输入、输出、整数系数和 scale 越界会返回带 `DECIMAL_*_LIMIT` code 的 `Err(Map)`；错误不会回显原始金额文本。

可运行示例：[decimal@1 示例](/source/examples/libraries/decimal.yan.txt)

## list@1

语言内核已经有 `list-map`、`list-filter`、`list-fold` 和 `sum`；`list@1` 补的是不执行回调的不可变结构操作：

| 函数 | 参数 | 结果 |
| --- | --- | --- |
| `list/reverse` | List | List |
| `list/append` | List, List | List |
| `list/contains?` | List, Any | Bool |
| `list/get` | List, Int | `Result<Any, Map>` |
| `list/take` | List, Int | `Result<List, Map>` |
| `list/drop` | List, Int | `Result<List, Map>` |
| `list/slice` | List, Int, Int | `Result<List, Map>` |

### 组合列表

`reverse` 和 `append` 直接返回新列表，原列表不会改变：

```lisp
(libraries (list 1))

(list/reverse (list 1 2 3))
; => (3 2 1)

(list/append (list 1 2) (list 3 4))
; => (1 2 3 4)

(list/contains? (list (map "id" 1)) (map "id" 1))
; => #t
```

`contains?` 比较完整 portable value 结构，不把值转成字符串，也不调用用户函数。

### 索引错误是业务数据

下标和范围可能来自请求参数，因此 `get`、`take`、`drop` 和 `slice` 返回 Result：

```lisp
(list/get (list "a" "b") 1)
; => (ok "b")

(list/slice (list 10 20 30 40) 1 3)
; => (ok (20 30))

(list/get (list "a") 5)
; => (err (map "code" "LIST_INDEX_OUT_OF_BOUNDS" "length" 1))
```

`slice` 使用半开区间 `[start, end)`。`take` 和 `drop` 接受 `0..=length`；负数、巨大整数和越界不会打穿请求，而会返回 `LIST_INDEX_OUT_OF_BOUNDS`、`LIST_COUNT_OUT_OF_BOUNDS` 或 `LIST_RANGE_OUT_OF_BOUNDS`。

可运行示例：[list@1 示例](/source/examples/libraries/list.yan.txt)

## map@1

内核已经提供 `map`、`get`、`get-or`、`has-key?` 和 `assoc`。`map@1` 补的是遍历、删除和合并：

| 函数 | 参数 | 结果 |
| --- | --- | --- |
| `map/size` | Map | Int |
| `map/keys` | Map | List |
| `map/values` | Map | List |
| `map/entries` | Map | List |
| `map/contains-value?` | Map, Any | Bool |
| `map/remove` | Map, Any | `Result<Map, Map>` |
| `map/merge-disjoint` | Map, Map | `Result<Map, Map>` |
| `map/merge-left` | Map, Map | Map |
| `map/merge-right` | Map, Map | Map |

### 确定性遍历

`keys`、`values` 和 `entries` 永远使用同一顺序：String 键在前，Symbol 键在后，同类键按字典序排列。

```lisp
(libraries (map 1))

(map/keys (map "b" 2 "a" 1))
; => ("a" "b")

(map/entries (map "b" 2 "a" 1))
; => (("a" 1) ("b" 2))
```

每个 entry 都是固定两个元素的列表。声明 `(list 1)` 后可以用 `list/get` 读取，也可以用内核 `list-fold` 继续处理。空 Map 的投影返回空列表。

### 在名字里写明冲突策略

配置覆盖通常保留右侧，默认值合并通常保留左侧；互不重名的数据则应拒绝冲突：

```lisp
(map/merge-right
  (map "timeout" 1000 "retries" 2)
  (map "timeout" 3000))
; => (map "retries" 2 "timeout" 3000)

(map/merge-disjoint
  (map "id" 1)
  (map "id" 2))
; => (err (map "code" "MAP_KEY_CONFLICT" "conflicts" 1))
```

`merge-left` 和 `merge-right` 直接返回 Map。`merge-disjoint` 返回 Result，因为冲突是调用者可以处理的业务数据。错误只报告冲突数量，不回显可能敏感的键。

`remove` 对不存在的 String/Symbol 键是幂等的；非键类型返回 `MAP_INVALID_KEY`：

```lisp
(map/remove (map "a" 1 "b" 2) "a")
; => (ok (map "b" 2))
```

可运行示例：[map@1 示例](/source/examples/libraries/map.yan.txt)

## encoding@1

`encoding@1` 在 String 与它的 UTF-8 字节编码之间做严格、确定性的转换：

| 函数 | 参数 | 结果 |
| --- | --- | --- |
| `encoding/base64-encode-text` | String | `Result<String, Map>` |
| `encoding/base64-decode-text` | String | `Result<String, Map>` |
| `encoding/hex-encode-text` | String | `Result<String, Map>` |
| `encoding/hex-decode-text` | String | `Result<String, Map>` |

### Base64 是严格规范格式

```lisp
(libraries (encoding 1))

(encoding/base64-encode-text "衍术🦀")
; => (ok "6KGN5pyv8J+mgA==")

(encoding/base64-decode-text "6KGN5pyv8J+mgA==")
; => (ok "衍术🦀")
```

v1 使用 RFC 4648 标准字母表，要求正确的 `=` padding，并检查最后未使用的 bit 必须为零。它不接受 URL-safe 的 `-` / `_`，也不接受省略 padding 的多重写法：

```lisp
(encoding/base64-decode-text "Zg")
; => (err (map "code" "ENCODING_INVALID_BASE64" "offset" 2))
```

如果外部协议使用 Base64URL，应等待独立、明确命名的契约，不能把两种格式混在一个“宽松解码”函数里。

### Hex 输出始终小写

```lisp
(encoding/hex-encode-text "AI")
; => (ok "4149")

(encoding/hex-decode-text "E8A18D")
; => (ok "衍")
```

Hex 解码接受 `a..f` 和 `A..F`，但编码始终输出小写。Base64/Hex 解码得到的字节还必须构成有效 UTF-8；任意二进制数据不会被假装成 String，而会返回 `ENCODING_INVALID_UTF8`。未来加入 `Bytes` 时会使用新的显式 API。

常见错误码：

| code | 含义 |
| --- | --- |
| `ENCODING_INVALID_BASE64` | Base64 字母、padding、长度或尾位不规范 |
| `ENCODING_INVALID_HEX` | Hex 长度为奇数或包含非十六进制字符 |
| `ENCODING_INVALID_UTF8` | 解码后的字节不是 UTF-8 |
| `ENCODING_INPUT_LIMIT` / `ENCODING_OUTPUT_LIMIT` | 输入或预测输出超过 1 MiB |

可运行示例：[encoding@1 示例](/source/examples/libraries/encoding.yan.txt)

## 资源与失败边界

标准库调用与普通表达式共享 guest fuel。每个操作的计费模型属于版本化契约；输入越长、输出越大或集合项越多，消耗越高。

文本结果最多 1 MiB。split 结果还受 10,000 个 portable 节点上限约束。摘要按输入 UTF-8 字节数计费，输出固定为 64 或 128 个 ASCII 字符。JSON 输入、输出和单个字符串最多 1 MiB，最多 10,000 个节点、64 层和 65,536 位整数；解析与序列化都在昂贵工作前扣 fuel。Decimal scale 最多 1,024，文本最多 20,002 bytes，系数最多 65,536 bits；重标度按 scale 差值计费，并在乘以十的幂之前预检结果。List 与 Map 的遍历和被复制结果都进入 fuel；append、entries 与 merge 在分配前检查结果是否仍满足 portable value 包络。Encoding 按输入和预测输出字节计费，并在 Base64/Hex 放大分配前检查 1 MiB 输出上限。后端在分配放大结果前检查上限，失败时返回稳定诊断或显式 Result，而不是继续占用宿主内存。

## Library 与 capability

| | Library | Capability |
| --- | --- | --- |
| 用途 | 纯文本、编码、确定性算法 | KV、clock、log 等外部效果 |
| 声明 | `(libraries (text 2) (math 1) (digest 1) (json 1) (decimal 1) (list 1) (map 1) (encoding 1))` | `(capabilities kv clock)` |
| 宿主状态 | 不接触 | 通过窄接口显式接触 |
| 效果闭包 | 不进入 | 进入静态 capability 闭包 |

guest 只能选择可信契约的名称与版本，不能指定 crates.io 包、动态库路径或 provider。宿主如何用 safe Rust、隔离进程或 WASM 实现契约，不会扩大 guest 可见 API。

继续阅读 [语法](/language/syntax)、[能力与副作用](/language/capabilities)和[安全模型](/evolution/security)。
