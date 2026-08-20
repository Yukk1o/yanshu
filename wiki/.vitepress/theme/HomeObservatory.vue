<template>
  <main class="observatory-home">
    <section class="observatory-hero" aria-labelledby="observatory-title">
      <div class="observatory-hero__copy">
        <p class="observatory-kicker">
          <span>AI-NATIVE LANGUAGE</span><span>CODE IS DATA</span>
        </p>
        <h1 id="observatory-title">
          让程序成为<br />
          <em>AI 可理解、可验证、</em><br />
          可继续演化的数据。
        </h1>
        <p class="observatory-deck">
          源码不是一次性文本，而是可检查、可生成、可执行的 AST。
          LLM 负责提出候选；解释器、测试门禁和版本指针决定什么能进入运行时。
          本项目由 AI 编程代理大量协助生成，可能包含大量 Bug，仅供实验研究，不用于生产。
        </p>
        <div class="observatory-actions" aria-label="快速入口">
          <a class="observatory-action observatory-action--primary" :href="withBase('/guide/quickstart')">
            5 分钟运行语言 <span aria-hidden="true">→</span>
          </a>
          <a class="observatory-action" :href="withBase('/evolution/review-ai-change')">
            审查 AI 改动 <span aria-hidden="true">↗</span>
          </a>
        </div>
      </div>

      <figure class="evolution-trace" aria-labelledby="trace-caption">
        <figcaption id="trace-caption">
          <span>EVOLUTION TRACE</span>
          <strong><i aria-hidden="true"></i> SIGNAL ONLINE</strong>
        </figcaption>
        <ol>
          <li>
            <span>01 / SOURCE</span>
            <strong>可读的 <code>.yan</code> 源码</strong>
            <small>当前已激活版本</small>
          </li>
          <li>
            <span>02 / AST</span>
            <strong>代码即结构化数据</strong>
            <small>Reader → Parser → Program</small>
          </li>
          <li class="evolution-trace__candidate">
            <span>03 / CANDIDATE</span>
            <strong>LLM 只提交候选</strong>
            <small>UNTRUSTED / 不直接生效</small>
          </li>
          <li>
            <span>04 / GATE</span>
            <strong>解释器 + 测试 + 策略</strong>
            <small>失败即拒绝，不能绕过</small>
          </li>
          <li class="evolution-trace__active">
            <span>05 / ACTIVE</span>
            <strong>内容寻址的运行版本</strong>
            <small>固定 hash / 可回滚</small>
          </li>
        </ol>
        <p><b>AI ≠ 裁判</b>。模型可以提出未来，只有宿主门禁能改变现实。</p>
      </figure>

      <div class="runtime-signal" aria-label="语言核心状态">
        <div><span>SOURCE MODEL</span><strong>S-expression → AST</strong></div>
        <div><span>SEMANTICS</span><strong>bounded interpreter</strong></div>
        <div><span>EVOLUTION</span><strong>test-gated</strong></div>
        <div class="runtime-signal__locked"><span>PRODUCTION GATE</span><strong>LOCKED</strong></div>
      </div>
    </section>

    <section class="observatory-section" aria-labelledby="checkpoint-title">
      <header class="observatory-section__head">
        <p>01 / LANGUAGE CORE</p>
        <h2 id="checkpoint-title">为 AI 协作而设计的语言核心</h2>
        <span>生成、验证和发布各自拥有明确边界，不把模型接进任意执行器。</span>
      </header>

      <div class="checkpoint-ledger">
        <article>
          <span class="checkpoint-ledger__index">A</span>
          <div>
            <h3>代码即数据</h3>
            <p>S 表达式直接映射成受限 AST。工具和模型可以检查结构、生成完整候选，宿主不需要执行任意原生源码。</p>
          </div>
          <span class="observatory-badge observatory-badge--done">已验证</span>
        </article>
        <article>
          <span class="checkpoint-ledger__index">B</span>
          <div>
            <h3>函数式默认</h3>
            <p>词法闭包、不可变值、显式 Result 和受控顺序表达式，让行为更容易比较、重放和生成测试。</p>
          </div>
          <span class="observatory-badge observatory-badge--done">已验证</span>
        </article>
        <article>
          <span class="checkpoint-ledger__index">C</span>
          <div>
            <h3>受限解释与能力</h3>
            <p>fuel、调用深度和输入边界限制执行；文件、网络与密钥默认不可见，KV、clock、log 必须由程序声明并由宿主注入。</p>
          </div>
          <span class="observatory-badge observatory-badge--done">已实现</span>
        </article>
        <article>
          <span class="checkpoint-ledger__index">D</span>
          <div>
            <h3>业务契约进入语言</h3>
            <p>Schema、路由、统一错误和 Library Backend 都是可审查结构；AI 的改动因此能按外部契约审查，而不只看实现文本。</p>
          </div>
          <span class="observatory-badge observatory-badge--done">已实现</span>
        </article>
      </div>
      <a class="observatory-inline-link" :href="withBase('/language/paradigms')">
        深入语言范式与取舍 <span aria-hidden="true">→</span>
      </a>
    </section>

    <section class="observatory-section language-sample" aria-labelledby="sample-title">
      <header class="observatory-section__head">
        <p>02 / THE LANGUAGE</p>
        <h2 id="sample-title">一段源码，就是一棵可验证的树</h2>
        <span>人类读业务意图，AI 读稳定结构，解释器只接受封闭语义。</span>
      </header>
      <div class="language-sample__layout">
        <pre aria-label="衍术折扣规则示例"><code><span>(program</span>
  (name price-rule)
  (version 1)
  (capabilities)

  (def decide
    (fn (price)
      (if (> price 100)
          (ok "review")
          (ok "accept"))))

  (export decide)<span>)</span></code></pre>
        <dl>
          <div><dt>PROGRAM</dt><dd>名字、版本、权限和公开入口都在 AST 中。</dd></div>
          <div><dt>FUNCTION</dt><dd>词法闭包与不可变数据让行为容易重放。</dd></div>
          <div><dt>RESULT</dt><dd>预期失败是显式数据，不冒充宿主崩溃。</dd></div>
          <div><dt>REVIEW</dt><dd>先审契约与场景，再审括号里的实现。</dd></div>
        </dl>
      </div>
      <a class="observatory-inline-link" :href="withBase('/language/syntax')">
        逐段读懂这段语言 <span aria-hidden="true">→</span>
      </a>
    </section>

    <section class="observatory-section observatory-section--routes" aria-labelledby="routes-title">
      <header class="observatory-section__head">
        <p>03 / LANGUAGE MAP</p>
        <h2 id="routes-title">按语言概念进入</h2>
        <span>先读你要使用或审查的契约，不必先顺序学完所有括号。</span>
      </header>

      <nav class="observation-routes" aria-label="文档阅读路径">
        <a :href="withBase('/language/syntax')"><span>01</span><strong>语法</strong><p>从完整 `.yan` 程序读懂 form、函数与控制流。</p><em>开始阅读 →</em></a>
        <a :href="withBase('/language/data-model')"><span>02</span><strong>数据模型</strong><p>理解 BigInt、List、Map、Result 与 JSON 边界。</p><em>查看值系统 →</em></a>
        <a :href="withBase('/backend/schema-errors')"><span>03</span><strong>Schema</strong><p>把输入约束、默认值和稳定错误变成语言结构。</p><em>定义契约 →</em></a>
        <a :href="withBase('/language/standard-library')"><span>04</span><strong>标准库</strong><p>通过版本化契约选择能力，不让 guest 任意加载依赖。</p><em>查看 Library Backend →</em></a>
        <a :href="withBase('/backend/web')"><span>05</span><strong>Web DSL</strong><p>声明路由、事务 handler 与统一 JSON 响应。</p><em>构建服务 →</em></a>
        <a :href="withBase('/evolution/lifecycle')"><span>06</span><strong>AI 演化</strong><p>让模型提交候选，经完整验证后再显式晋升。</p><em>理解生命周期 →</em></a>
        <a :href="withBase('/evolution/security')"><span>07</span><strong>安全</strong><p>检查能力、预算、密钥、版本固定与生产缺口。</p><em>查看信任边界 →</em></a>
      </nav>
    </section>

    <section class="evolution-boundary" aria-labelledby="boundary-title">
      <div>
        <p class="observatory-kicker"><span>RUNTIME BOUNDARY</span></p>
        <h2 id="boundary-title">持续演化，<br /><em>不等于无边界改写。</em></h2>
      </div>
      <div class="evolution-boundary__body">
        <p>Provider 不能修改测试、触碰活动指针或读取 API key。测试通过是必要条件，不是业务意图的替代品。</p>
        <p class="evolution-boundary__warning"><span>PRODUCTION GATE / LOCKED</span> 当前仍缺生产隔离、运营与灰度切换能力。</p>
        <a class="observatory-action" :href="withBase('/evolution/lifecycle')">
          查看完整演化边界 <span aria-hidden="true">→</span>
        </a>
      </div>
    </section>
  </main>
</template>

<script setup lang="ts">
import { withBase } from 'vitepress'
</script>
