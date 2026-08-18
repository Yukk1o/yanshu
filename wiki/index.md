---
layout: home

hero:
  name: AI-Evolve
  text: 把程序当作数据，但把上线权留给测试
  tagline: 一份面向 Go / Rust 开发者的中文工程手册。先看懂宿主、客体和安全边界，再进入 Lisp 语法与 AI 演化闭环。
  actions:
    - theme: brand
      text: 5 分钟跑起来
      link: /guide/quickstart
    - theme: alt
      text: 先看架构图
      link: /guide/architecture

features:
  - title: 用熟悉的语言理解
    details: 每个核心概念都映射到 Go interface、Rust enum / trait / Result，而不是要求你先成为 Lisp 专家。
  - title: 链接真实源码
    details: 构建时从仓库同步受控源码快照，文档中的链接始终对应本次构建使用的实现。
  - title: 明确 AI 的权限
    details: AI 只能提出候选源码；Reader、解释器、测试、版本库与晋升策略由可信宿主掌握。
  - title: 能跑的 Web 原型
    details: 已有路由、Schema、统一错误、事务 KV、HTTP 服务、版本热切换和响应式任务管理页面。
---

## 先记住一句话

AI-Evolve 不是“让 AI 直接改线上进程”，而是：**让 AI 像开发者一样提交候选版本，再由不可修改的测试和宿主策略决定它能否成为活动版本。**

<div class="concept-map">
  <div><strong>Racket host</strong>相当于今天的 Go/Rust 服务进程，负责解析、执行、网络、存储与权限。</div>
  <div><strong>.ail guest</strong>相当于受限的业务规则模块，采用 Lisp S 表达式，能被当成普通数据分析和生成。</div>
  <div><strong>LLM provider</strong>相当于只会提交 PR 的外部开发者，没有测试裁判权，也没有直接发布权。</div>
  <div><strong>Version store</strong>相当于内容寻址的制品库加活动指针，支持按请求固定版本和快速回滚。</div>
</div>

不想先学 Lisp？直接进入[项目是什么](/guide/what-is)，其中用 Go/Rust 伪代码解释完整执行链。
