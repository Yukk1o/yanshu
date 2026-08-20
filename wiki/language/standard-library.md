# 标准库与 Library Backend

Yanshu 把“语言可调用的 API”与“宿主怎样实现它”分开。guest 只声明版本化标准库契约，不能直接指定 crates.io 包、动态库路径或任意函数名。

## 声明标准库

```lisp
(program
  (name text-demo)
  (version 1)
  (capabilities)
  (libraries (text 1))

  (def summarize
    (fn (value)
      (map "length" (text/length value)
           "has-ai" (text/contains? value "AI"))))

  (export summarize))
```

`(libraries (text 1))` 选择的是 `text@1` 契约。未知库、错误版本、重复声明或占用 `text/...` 命名空间都会在解析阶段失败。

## `text@1` API

| 函数 | 参数 | 结果 |
| --- | --- | --- |
| `text/length` | String | Unicode scalar 数量的 Int |
| `text/starts-with?` | String, String | Bool |
| `text/ends-with?` | String, String | Bool |
| `text/contains?` | String, String | Bool |
| `text/replace` | String, String, String | String |

```lisp
(text/length "AI语言")
(text/starts-with? "AI language" "AI")
(text/replace "AI language" "AI" "机器")
```

可运行示例见 [examples/libraries/text.yan](/source/examples/libraries/text.yan.txt)。v0.9 的契约在独立 [yanshu-library contract](/source/rust/crates/yanshu-library/src/contract.rs.txt)，Rust 实现在 [text backend](/source/rust/crates/yanshu-library/src/text.rs.txt)，解释器只负责注册和调用边界。

## 为什么不直接导入 Cargo crate

如果 guest 能按字符串加载任意依赖，模型就可能扩大权限、改变语义或引入不可审计的供应链。Library Backend 使用三层边界：

```text
.yan portable API
        │
        ▼
versioned contract
函数名 / 参数 / 结果 / fuel / portable value
        │
        ▼
trusted backend implementation
Rust crate / 隔离 sidecar / WASM（按宿主策略选择）
```

contract 固定可见表面；backend 不能增加 guest 可调用函数、改变参数类型、降低 fuel 计费或返回 Closure/Primitive 等非 portable 值。

## Library 与 capability 的区别

| 维度 | Library | Capability |
| --- | --- | --- |
| 典型用途 | 纯文本、编码、确定性算法 | KV、clock、log 等外部效果 |
| 声明位置 | `(libraries (text 1))` | `(capabilities kv clock)` |
| 是否应确定性 | 是 | 不一定 |
| 是否接触宿主状态 | 不应 | 通过窄接口显式接触 |
| 测试策略 | 相同输入必须得到 portable 结果 | 使用固定时钟、内存事务等 adapter |

## v0.9 Rust Backend 已实现

宿主可以实现安全 Rust trait，并显式注册：

```rust
trait LibraryBackend: Send {
    fn descriptor(&self) -> BackendDescriptor;
    fn invoke(
        &mut self,
        operation: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<LibraryValue>;
}
```

`RustTextBackend` 已经替代解释器内部的 text 特判。注册时 operation 集合必须与可信 contract 精确相等；provider 名只进入脱敏诊断，guest 看不到也不能选择。

## backend 的验证责任

宿主调用 backend 前后都要检查：

1. 程序确实声明了精确库版本；
2. 操作属于 contract；
3. 参数数量和类型正确；
4. 在进入 backend **之前**扣除 contract 计算的 fuel；不足时 backend 一次也不会被调用；
5. 结果满足节点数、深度、字符串长度和 portable value 限制；
6. backend 失败转换成稳定诊断，而不是暴露宿主堆栈。

## crates.io、FFI 与第三方生态路线

::: warning 当前状态
`yanshu-library` 和可替换 Rust Backend 已落地，但 workspace 仍是 `publish = false`；项目没有稳定 C ABI、动态库 ABI 或让 guest 任意加载 crates.io 的入口。
:::

合理的生态顺序是：

1. 先稳定 `Program`、portable `Value`、诊断和 Library Contract 的版本语义；
2. 把可复用 Rust crate 分层发布，明确 MSRV、feature 和安全策略；
3. 扩充 Library Backend conformance suite 与更多确定性 contract；
4. 如需 FFI，使用显式句柄、长度字段、错误对象和 BigInt 编码，不直接暴露 Rust enum 布局；
5. 外部 backend 先运行在可终止的进程或 WASM 沙箱，再考虑进程内动态加载；
6. 所有新库都必须有预算、确定性、portable value 和供应链审查。

继续阅读 [安全模型](/evolution/security) 和 [Rust 宿主与生态路线](/development/rust-roadmap)。
