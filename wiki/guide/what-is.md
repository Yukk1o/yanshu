# AI-Evolve 是什么语言

AI-Evolve 是一门把程序当作**可验证数据**的实验性通用语言。它不复制 Rust 的系统编程定位；目标是让 AI 可以持续生成应用代码，同时让候选容易理解、容易验证，又不能绕过测试和发布边界直接进入运行时。

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
| 模块 hash + 密封 Bundle | 多文件仍然形成一个可验证、可回滚的依赖闭包 |
| package hash + 精确 lock | 跨包复用不会退回开发路径或漂移依赖 |
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

## 当前适合做什么

当前最合适的场景是小型、可测试、契约清楚的业务逻辑：

- 价格、折扣、资格、路由等纯规则；
- 带 Schema 的 JSON API handler；
- 使用事务 KV 的小型 CRUD 服务；
- 需要模型提出候选、由完整场景验证后再晋升的程序；
- 研究 AST patch、结构化 diff 和只读审查视图。

完整案例包括覆盖 11 个有状态场景的[任务 CRUD 服务](/source/examples/tasks/service.ail.txt)，验证 v2 条件、集合、enum/union、校验成本和业务 Result 的[费用审批服务](/source/examples/expenses/service.ail.txt)，以及验证 v3 模块、封闭数据、模式匹配和内容密封的[多模块费用审批 Bundle](/language/modules-bundles)。这些是通用语言内核的验收程序，不是最终应用边界。

## 通用语言目标与当前边界

AI-Evolve v0.9 已有用户模块、typed 封闭数据、模式匹配、密封 Bundle、导出签名、静态 capability 闭包、Rust 风格只读审查、内容寻址包/锁文件和可替换 Rust Library Backend，但仍处于通用语言的安全内核阶段，还不是通用系统语言或公网生产框架。它尚无编译目标、并发、浮点数及通用文件/网络 API；宿主侧也仍缺少独立进程沙箱、正式数据库/PITR、异地备份、指标告警和 canary 自动化。

这些既是阶段性功能缺口，也是不可绕过的设计约束。新能力必须通过版本化语义、密封 Bundle 和明确 capability/effect 引入，不能让模型靠调用未知宿主函数越过边界。目标是安全的通用应用语言，而不是拥有环境权限和任意 `unsafe` 的系统语言。

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
