---
layout: home

hero:
  name: 衍术 Yanshu
  text: 为人类审查和 AI 协作设计的受限编程语言
  tagline: 用函数、不可变数据和显式能力编写业务程序；用类型、fuel 和测试门禁验证 AI 生成的候选代码。
  actions:
    - theme: brand
      text: 5 分钟上手
      link: /guide/quickstart
    - theme: alt
      text: 学习语法
      link: /language/syntax

features:
  - title: 程序即数据
    details: .yan 源码有简单、稳定的树形结构，容易生成、差异对比和机器检查。
  - title: 能力显式声明
    details: 程序默认看不到文件、网络、密钥和线程；KV、时钟与日志由宿主按需注入。
  - title: 可验证执行
    details: 语言 v4 提供类型/效果分析、有界解释与字节码 VM、密封 Bundle 和内容寻址包。
  - title: 开发者工具
    details: 已有 CLI、VS Code 扩展、LSP、Tree-sitter、只读 MCP 和 Rust 风格只读审查视图。
---

## 一个完整函数

```lisp
(program
  (name hello)
  (version 4)
  (capabilities)

  (signature greet (fn (string) string))
  (def greet
    (fn (name)
      (string-append "你好，" name)))

  (export greet))
```

将它保存为 `hello.yan`，用 CLI 编译并以 `["世界"]` 作为参数运行，会得到：

```json
{"ok":true,"result":"你好，世界"}
```

具体命令见[安装与 5 分钟上手](/guide/quickstart)。

## 推荐学习顺序

1. [认识衍术](/guide/what-is)，判断它是否适合你的问题。
2. [运行第一个程序](/guide/quickstart)，安装 CLI 和 VS Code 扩展。
3. 依次学习[语法](/language/syntax)、[数据](/language/data-model)与[函数和 Result](/language/functions-results)。
4. 用[模块与 Bundle](/language/modules-bundles)组织程序，用[能力](/language/capabilities)接入受控副作用。
5. 跟随[费用审批实战](/guide/expense-app)和 [Web DSL](/backend/web)完成一个真实应用。

::: warning 当前状态
衍术 v0.12.0 是实验性软件，主要由 AI 编程代理协助生成，可能存在大量 Bug。它尚未生产就绪；不要将当前 HTTP 宿主直接暴露到公网，也不要用它处理关键或敏感业务。
:::
