# 项目是什么

AI-Evolve 是一个“小型语言 + 受限解释器 + 版本门禁”的实验。目标不是发明一种比 Go 或 Rust 更适合人类的大型通用语言，而是验证：**业务程序能否以结构化数据存在，让 AI 生成后继版本，同时不把运行权、测试权和发布权交给 AI。**

## 两种语言，不要混在一起

| 名称 | 当前选择 | 职责 | 将来是否替换 |
| --- | --- | --- | --- |
| 宿主语言 | Racket | Reader、Parser、解释器、HTTP、KV、测试、版本库、LLM 接口 | 计划迁到 Rust |
| 客体语言 | 项目自己的 `.ail` Lisp | 用户可编写、AI 可生成的业务规则 | 语义和文件格式应保持稳定 |

如果用 Go 来类比：Racket 部分像一个 Go 服务进程；`.ail` 程序像进程里加载的受限规则文件。如果用 Rust 来类比：`.ail` 源码被解析成 `enum Expr`，再由一个带预算的 `eval(&Expr, &mut Context)` 执行。

源码入口：[AST 定义](/source/src/ast.rkt.txt)、[Reader](/source/src/reader.rkt.txt)、[Parser](/source/src/parser.rkt.txt)、[解释器](/source/src/runtime.rkt.txt)。

## 一次普通请求发生了什么

```text
浏览器 / HTTP 客户端
        │ JSON 请求
        ▼
Racket HTTP host ──读取一次 active hash──► 版本库
        │                                      │
        │ service-request                      └─返回固定的 .ail 源码
        ▼
路由匹配 ─► Parser/AST ─► 受限解释器 ─► .ail handler
                                   │
                                   ├─显式 kv 能力（事务）
                                   ├─显式 clock 能力
                                   └─显式 log 能力
        │
        ▼
宿主验证响应形状 ─► 成功才提交 KV ─► JSON 响应
```

这里最重要的是“固定版本”：请求开始后就拿到一个确定的程序对象。即使另一个请求此时晋升了新版本，正在运行的请求也不会执行到一半换代码。

## Go / Rust 概念对照

| AI-Evolve | Go 中可以怎么理解 | Rust 中可以怎么理解 |
| --- | --- | --- |
| `ail-program` / AST | 一组 struct + interface 节点 | `Program` + `enum Expr` |
| `execute-export` | 调用注册表中的 handler | 对已校验 IR 求值 |
| `Ok` / `Err` 客体值 | `(value, err)` 的显式分支 | `Result<T, E>` |
| capability | 只注入所需的小 interface | 传入受限 trait object / capability token |
| fuel | 每执行一步减一的预算 | `&mut Budget`，耗尽返回诊断 |
| KV transaction | handler 成功后 `tx.Commit()` | `Transaction` 成功路径提交，失败丢弃 |
| source hash | 制品的 SHA-256 ID | 不可变 artifact ID |
| promote / rollback | 原子更新当前版本指针 | 受策略保护的 active pointer |

## 现在已经能做什么

- 写纯函数并用 JSON 案例测试；
- 声明 GET、POST、PUT、PATCH、DELETE 路由和路径参数；
- 读取请求的 params、query、headers、body；
- 用 Schema 校验字符串、整数、布尔、列表和封闭对象；
- 返回统一的成功响应和错误信封；
- 在事务 KV 中持久化 JSON 数据；
- 使用受宿主控制的时钟和结构化日志；
- 启动一个有并发数、大小、读取时间和执行时间限制的本地 HTTP 服务；
- 对候选版本运行完整测试，通过后注册、晋升或回滚；
- 调用 DeepSeek Chat Completions 或 OpenAI Responses 生成候选。

完整可运行案例是[任务 CRUD 服务](/source/examples/tasks/service.ail.txt)。

## 它目前不是什么

它不是公网生产框架，也不是通用 Lisp。当前没有认证授权、TLS、PostgreSQL、异步任务、包管理、静态类型系统、宏、文件上传、WebSocket 或独立进程沙箱。KV 是本地 JSON 原型；HTTP host 也是为了验证语义而写的有边界实现。

因此正确定位是：**可运行的本地业务后端原型，以及 Rust 重写前的语义实验室。** 生产化缺口见 [Web 后端与路由](/backend/web#当前边界)。

## 为什么客体语言还是 Lisp

Lisp 的括号不是为了让用户痛苦，而是让源码天然接近一棵树：

```lisp
(if (= user-type "vip")
    (- price (quotient price 10))
    price)
```

它几乎可以直接表示为：

```text
If(
  Call("=", [user_type, "vip"]),
  Call("-", [price, Call("quotient", [price, 10])]),
  price
)
```

这对 AI、Parser、静态检查和后续 AST patch 都很友好。你不必喜欢 Lisp 才能理解宿主项目；先用 [Go / Rust 视角看架构](/guide/architecture)，需要改业务规则时再查[语法入门](/language/syntax)。
