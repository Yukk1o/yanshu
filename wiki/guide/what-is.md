# AI-Evolve 是什么语言

AI-Evolve 是一门把程序当作**可验证数据**的实验性语言。它不是要替代 Go 或 Rust 去编写所有软件，而是解决一个更具体的问题：当 AI 能持续生成代码时，怎样让候选代码容易理解、容易验证，又不能绕过测试和发布边界直接进入运行时。

它的核心承诺是：**AI 有提案权，语言宿主保留解释权、验证权和发布权。**

## 一眼看懂一个程序

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

括号只是表面形式。解析后它是一棵明确的 `Program` / `Expression` 树，近似下面的 Rust 数据：

```rust
Program {
    name: "discount",
    capabilities: [],
    definitions: [Definition {
        name: "calculate-discount",
        value: Expr::Function { /* ... */ },
    }],
    exports: ["calculate-discount"],
}
```

这就是“代码即数据”：模型可以生成完整程序，工具可以比较 AST，解释器只执行语言允许的节点。源码示例见 [discount/v2.ail](/source/examples/discount/v2.ail.txt)，结构定义见 [ail-syntax AST](/source/rust/crates/ail-syntax/src/ast.rs.txt)。

## 语言为 AI 做了哪些取舍

| 取舍 | 对 AI 和审查者的价值 |
| --- | --- |
| S 表达式对应 AST | 少一层复杂语法，结构边界明显 |
| 小而封闭的 form 集合 | 模型不能凭空发明宿主语法 |
| 函数式默认、不可变值 | 行为更容易重放、比较和测试 |
| Schema、route、capability 是语言结构 | API、数据与权限变化可以结构化审查 |
| 稳定诊断 code + JSON 输出 | 工具不必解析人类终端文案 |
| fuel、深度和输入上限 | 候选不能无限消耗解释器资源 |
| 内容哈希版本 + 显式晋升 | “生成成功”不会自动变成“正在运行” |

## 语言由哪些部分组成

```text
.ail 源码
   │
   ▼
受限 Reader ──► Parser ──► AST
                           │
               ┌───────────┴───────────┐
               ▼                       ▼
        有界解释器               结构化检查 / diff
               │
       ┌───────┼────────┐
       ▼       ▼        ▼
   纯函数    Web DSL   版本化标准库
               │
               ▼
      显式 capability 与事务
```

- **语言前端**只接受一个有边界的 S 表达式，验证程序、Schema、路由和导出。
- **解释器**执行自己的 AST，不执行任意宿主源码；fuel 和调用深度构成硬预算。
- **数据模型**包含任意精度整数、字符串、布尔、List、Map、Result 和闭包。
- **Web DSL**把 route、request、response、Schema 和统一错误变成语言契约。
- **Library Backend**用版本化 portable API 连接可信实现，不让 guest 任意加载包。
- **演化控制面**把候选、测试报告、内容哈希、active 指针和回滚分开。

## 它适合做什么

当前最合适的场景是小型、可测试、契约清楚的业务逻辑：

- 价格、折扣、资格、路由等纯规则；
- 带 Schema 的 JSON API handler；
- 使用事务 KV 的小型 CRUD 服务；
- 需要模型提出候选、由完整场景验证后再晋升的程序；
- 研究 AST patch、结构化 diff 和只读审查视图。

完整案例是[任务 CRUD 服务](/source/examples/tasks/service.ail.txt)，覆盖 11 个有状态场景。

## 它刻意不是什么

AI-Evolve 当前不是通用系统语言，也不是公网生产框架。它没有宏、可变变量、并发、用户模块、任意包导入、浮点数、文件和网络能力；宿主侧也仍缺少细粒度授权、独立进程沙箱、正式数据库/PITR、异地备份、指标告警和 canary 自动化。

这些限制不是临时藏起来的“功能缺失”，而是语言可信边界的一部分。新能力应该通过版本化语义和明确 capability 引入，不能让模型靠调用未知宿主函数越过边界。

## 人类怎样阅读它

不需要先成为 Lisp 专家。先记住：左括号后第一个词是操作，其余是参数。

```lisp
(if (= role "admin")
    (api-response 200 data)
    (api-error 403 "FORBIDDEN" "access denied"))
```

可以直接读成：

```rust
if role == "admin" {
    api_response(200, data)
} else {
    api_error(403, "FORBIDDEN", "access denied")
}
```

下一步读[语言范式](/language/paradigms)建立心智模型，或直接进入[语法入门](/language/syntax)。
