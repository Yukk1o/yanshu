import { defineConfig } from 'vitepress'

export default defineConfig({
  base: process.env.YANSHU_DOCS_BASE || '/',
  lang: 'zh-CN',
  title: '衍术 Yanshu',
  description: '衍术语言的学习指南、工具与 API 参考',
  appearance: true,
  cleanUrls: true,
  srcExclude: ['README.md'],
  lastUpdated: true,
  head: [
    ['meta', { name: 'theme-color', content: '#3451b2' }],
    ['meta', { name: 'viewport', content: 'width=device-width, initial-scale=1.0' }]
  ],
  markdown: {
    lineNumbers: true
  },
  themeConfig: {
    siteTitle: '衍术 Yanshu',
    nav: [
      { text: '快速开始', link: '/guide/quickstart' },
      { text: '语言指南', link: '/language/syntax' },
      { text: '构建应用', link: '/backend/web' },
      { text: '工具', link: '/development/vscode' },
      { text: 'CLI 参考', link: '/reference/cli' }
    ],
    sidebar: [
      {
        text: '1. 认识衍术',
        items: [
          { text: '语言是什么', link: '/guide/what-is' },
          { text: '安装与 5 分钟上手', link: '/guide/quickstart' }
        ]
      },
      {
        text: '2. 语法与数据',
        items: [
          { text: '语法入门', link: '/language/syntax' },
          { text: '数据模型', link: '/language/data-model' },
          { text: '函数、控制流与 Result', link: '/language/functions-results' }
        ]
      },
      {
        text: '3. 组织与安全边界',
        items: [
          { text: '模块、数据类型与 Bundle', link: '/language/modules-bundles' },
          { text: '能力与副作用', link: '/language/capabilities' },
          { text: '类型、效果与只读审查', link: '/language/types-effects-review' }
        ]
      },
      {
        text: '4. 工具与编辑器',
        items: [
          { text: 'VS Code 扩展', link: '/development/vscode' },
          { text: '格式化与代码导航', link: '/development/formatter' },
          { text: '其他编辑器接入 LSP', link: '/development/lsp' },
          { text: 'Codex / Claude / OpenCode', link: '/development/mcp' }
        ]
      },
      {
        text: '5. 构建真实应用',
        items: [
          { text: 'Schema 与业务错误', link: '/backend/schema-errors' },
          { text: '费用审批实战', link: '/guide/expense-app' },
          { text: 'Web DSL 与路由', link: '/backend/web' }
        ]
      },
      {
        text: '6. 参考与进阶',
        items: [
          { text: 'CLI 参考', link: '/reference/cli' },
          { text: '标准库', link: '/language/standard-library' },
          { text: '包与锁文件', link: '/language/packages-lockfiles' },
          { text: '字节码与 WASM', link: '/language/bytecode-wasm' },
          { text: 'AI 候选、审查与晋升', link: '/evolution/lifecycle' }
        ]
      },
      {
        text: '贡献者',
        collapsed: true,
        items: [
          { text: '参与语言开发', link: '/development/contributing' }
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
      message: '当前为实验性 v0.12.0；请先在非生产环境评估。',
      copyright: 'Yanshu language documentation'
    }
  }
})
