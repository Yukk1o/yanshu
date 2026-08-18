import { defineConfig } from 'vitepress'

export default defineConfig({
  lang: 'zh-CN',
  title: 'AI-Evolve Wiki',
  description: '让程序成为 AI 可理解、可验证、可继续演化的数据',
  appearance: 'force-dark',
  cleanUrls: true,
  lastUpdated: true,
  head: [
    ['meta', { name: 'theme-color', content: '#071017' }],
    ['meta', { name: 'viewport', content: 'width=device-width, initial-scale=1.0' }]
  ],
  markdown: {
    lineNumbers: true
  },
  themeConfig: {
    siteTitle: 'AI-Evolve / 语言观测台',
    nav: [
      { text: '语言概览', link: '/guide/what-is' },
      { text: '语法', link: '/language/syntax' },
      { text: '数据模型', link: '/language/data-model' },
      { text: 'Web DSL', link: '/backend/web' },
      { text: 'AI 演化', link: '/evolution/lifecycle' },
      { text: '安全', link: '/evolution/security' }
    ],
    sidebar: [
      {
        text: '认识 AI-Evolve',
        items: [
          { text: '语言是什么', link: '/guide/what-is' },
          { text: '5 分钟上手', link: '/guide/quickstart' },
          { text: '语言范式', link: '/language/paradigms' }
        ]
      },
      {
        text: '语言手册',
        items: [
          { text: '语法入门', link: '/language/syntax' },
          { text: '数据模型', link: '/language/data-model' },
          { text: '模块、数据类型与 Bundle', link: '/language/modules-bundles' },
          { text: 'Schema 与统一错误', link: '/backend/schema-errors' },
          { text: '标准库与 Library Backend', link: '/language/standard-library' },
          { text: 'Web DSL 与路由', link: '/backend/web' }
        ]
      },
      {
        text: 'AI 演化与安全',
        items: [
          { text: '候选、验证、晋升与回滚', link: '/evolution/lifecycle' },
          { text: '安全模型与能力边界', link: '/evolution/security' },
          { text: '如何审查 AI 生成的改动', link: '/evolution/review-ai-change' }
        ]
      },
      {
        text: '实现与工具',
        items: [
          { text: '实现架构', link: '/guide/architecture' },
          { text: 'CLI 参考', link: '/reference/cli' },
          { text: '源码地图', link: '/reference/source-map' },
          { text: 'Rust 宿主与生态路线', link: '/development/rust-roadmap' },
          { text: 'Git 分支工作流', link: '/development/git-workflow' },
          { text: '维护 Wiki', link: '/development/wiki' }
        ]
      }
    ],
    search: {
      provider: 'local'
    },
    outline: {
      level: [2, 3],
      label: '本页目录'
    },
    docFooter: {
      prev: '上一页',
      next: '下一页'
    },
    lastUpdated: {
      text: '最后更新'
    },
    returnToTopLabel: '返回顶部',
    sidebarMenuLabel: '目录',
    darkModeSwitchLabel: '外观',
    lightModeSwitchTitle: '切换到浅色模式',
    darkModeSwitchTitle: '切换到深色模式',
    footer: {
      message: 'AI 负责提出候选，语言门禁负责决定什么能够运行。',
      copyright: 'AI-Evolve language documentation'
    }
  }
})
