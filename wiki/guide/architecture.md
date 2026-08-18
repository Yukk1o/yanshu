# 架构导览

如果你熟悉 Go / Rust，可以把项目看成“一个加载受限 IR 的应用服务器”。Lisp 只是输入格式，真正的系统由几个边界清楚的宿主模块构成。

## 总体数据流

```text
                 不可信输入
       ┌─────────────────────────────┐
       │ .ail source   LLM proposal  │
       └──────┬──────────────┬───────┘
              │              │
              ▼              │
     Reader → Parser → typed AST
              │              │
              ▼              │
       bounded interpreter    │
              │              │
              ▼              │
       complete test suites ◄─┘
              │ passed
              ▼
       immutable version store
              │ explicit promote
              ▼
          active pointer
              │ pinned per request
              ▼
 HTTP → route → handler → validated response → commit transaction

     上面所有箭头的规则都由可信宿主掌握
```

## 四层结构

### 1. 语言前端：Reader、Parser、AST

- [reader.rkt](/source/src/reader.rkt.txt) 只接受一个 S 表达式，关闭 `#lang`、reader extension、图结构等宿主逃逸入口，并限制节点数和深度。
- [parser.rkt](/source/src/parser.rkt.txt) 验证顶层声明、能力、Schema、路由、导出和表达式，生成显式 AST。
- [ast.rkt](/source/src/ast.rkt.txt) 定义 `ail-program`、`ail-route`、Schema 节点与表达式节点。

Rust 迁移时，这层很自然地对应：

```rust
enum Expr {
    Lit(Value),
    Var(Symbol),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Let(Vec<Binding>, Box<Expr>),
    Fn(Vec<Symbol>, Box<Expr>),
    Do(Vec<Expr>),
    Call(Box<Expr>, Vec<Expr>),
}
```

关键点是：项目不调用 Racket `eval`。客体源码只会变成自己的 AST，因而迁移宿主不会改变 `.ail` 语义。

### 2. 执行内核：解释器与能力

[runtime.rkt](/source/src/runtime.rkt.txt) 是树遍历解释器。`evaluate` 对 AST 做模式分派，`apply-callable` 只接受项目闭包或可信 primitive。

执行上下文包含：

- `fuel`：每次求值和调用都会消耗；
- `maximum-depth`：限制调用深度；
- `logger`：只有声明 `log` 时才安装；
- capability bindings：KV、clock 由宿主按需注入。

Go 版本可以把 capability 写成窄接口，Rust 版本可以写成 trait：

```rust
trait Clock { fn now_ms(&self) -> BigInt; }
trait KvTx {
    fn get(&self, key: &str) -> Option<Value>;
    fn put(&mut self, key: String, value: Value);
}
```

客体代码拿不到文件句柄、socket、数据库连接或 API key，只能调用声明并注入的 primitive。

### 3. Web 宿主：HTTP、路由、事务

- [http-server.rkt](/source/src/http-server.rkt.txt) 负责 TCP、HTTP 解析、请求限制、超时、JSON 和静态测试页面；
- [service.rkt](/source/src/service.rkt.txt) 匹配路由、构造 guest request、执行 handler、验证 response；
- [kv-store.rkt](/source/src/kv-store.rkt.txt) 为每个请求建立 working copy，只有 handler 正常完成且响应合法才提交；
- [schema.rkt](/source/src/schema.rkt.txt) 递归校验输入并生成稳定 issue 列表。

这正像传统后端的 adapter / application / repository 分层，只是业务 application layer 由 `.ail` 表达。

### 4. 演化控制面：Provider、测试、版本

- [evolver.rkt](/source/src/evolver.rkt.txt) 把当前源码和观察发送给 LLM，严格验证其 JSON 响应；
- [evolution-loop.rkt](/source/src/evolution-loop.rkt.txt) 解析候选、运行测试、注册版本，并在宿主明确要求时尝试晋升；
- [service-test-suite.rkt](/source/src/service-test-suite.rkt.txt) 顺序运行有状态 Web 场景；
- [version-store.rkt](/source/src/version-store.rkt.txt) 按 SHA-256 保存不可变源码和元数据，维护 active 指针和事件日志；
- [service-deployment.rkt](/source/src/service-deployment.rkt.txt) 组合服务部署、活动版本加载和服务演化。

## 请求路径：一步一步

1. HTTP host 接收连接，并在大小和截止时间内解析请求。
2. program loader 读取一次 active source，解析成 `ail-program`；这个对象固定到请求结束。
3. service 层按 method + path 匹配静态 route，提取 `:id` 等参数。
4. 如果程序声明 `kv`，宿主创建请求级事务和四个 KV primitive。
5. 解释器以 request Map 作为唯一参数调用导出的 handler。
6. service 层检查返回值必须恰有 `status`、`headers`、`body`，并验证 header 与 JSON 序列化。
7. 没有诊断时提交事务；异常、fuel 耗尽、超时或非法响应都丢弃事务。
8. host 写回 HTTP 响应，并只记录脱敏观察。

## 信任边界

| 可信内核 | 不可信数据 |
| --- | --- |
| Reader / Parser / AST 规则 | `.ail` 源码 |
| 解释器、fuel、深度与超时 | LLM 返回的候选 |
| capability dispatcher | HTTP 请求 body |
| 测试集与比较逻辑 | 候选写下的说明 |
| 版本库与晋升策略 | 运行观察中的业务失败 |

这就是项目与“让模型在运行时直接 rewrite 当前函数”的本质差异。动态生成仍然存在，但新代码先成为不可变候选制品，不会原地污染活动版本。

## 为什么先用 Racket、再迁 Rust

Racket 让 Reader、S 表达式和树遍历解释器能够快速验证；Rust 更适合长期宿主，因为它能提供清晰的数据类型、错误枚举、资源所有权、并发模型和单二进制部署。迁移边界从一开始就被写进[设计文档](/source/docs/design.md.txt)：`.ail`、JSON 测试、诊断代码、版本元数据和一致性测试保持不变，只替换宿主实现。

具体模块映射见 [Rust 迁移路线](/development/rust-roadmap)。
