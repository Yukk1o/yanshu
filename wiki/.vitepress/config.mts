import { defineConfig } from 'vitepress'

export default defineConfig({
  lang: 'zh-CN',
  title: 'AI-Evolve Wiki',
  description: '用 Go / Rust 思维读懂可测试、可晋升、可回滚的 AI 编程语言实验',
  cleanUrls: true,
  lastUpdated: true,
  head: [
    ['meta', { name: 'theme-color', content: '#17324d' }],
    ['meta', { name: 'viewport', content: 'width=device-width, initial-scale=1.0' }]
  ],
  markdown: {
    lineNumbers: true
  },
  themeConfig: {
    siteTitle: 'AI-Evolve / 工程手册',
    nav: [
      { text: '开始', link: '/guide/quickstart' },
      { text: '语言', link: '/language/syntax' },
      { text: 'Web 后端', link: '/backend/web' },
      { text: 'AI 演化', link: '/evolution/lifecycle' },
      { text: '审查 AI 改动', link: '/evolution/review-ai-change' },
      { text: 'CLI', link: '/reference/cli' }
    ],
    sidebar: [
      {
        text: '先建立全局认识',
        items: [
          { text: '项目是什么', link: '/guide/what-is' },
          { text: '5 分钟上手', link: '/guide/quickstart' },
          { text: '架构导览', link: '/guide/architecture' }
        ]
      },
      {
        text: '读懂这门语言',
        items: [
          { text: '语法入门', link: '/language/syntax' },
          { text: 'Web 后端与路由', link: '/backend/web' },
          { text: 'Schema 与统一错误', link: '/backend/schema-errors' }
        ]
      },
      {
        text: '理解演化与安全',
        items: [
          { text: '候选、测试、晋升与回滚', link: '/evolution/lifecycle' },
          { text: '如何审查 AI 生成的改动', link: '/evolution/review-ai-change' }
        ]
      },
      {
        text: '操作与开发',
        items: [
          { text: 'CLI 参考', link: '/reference/cli' },
          { text: '源码地图', link: '/reference/source-map' },
          { text: 'Git 分支工作流', link: '/development/git-workflow' },
          { text: 'Rust 迁移路线', link: '/development/rust-roadmap' },
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
      message: '宿主负责安全边界，AI 只负责提交候选。',
      copyright: 'AI-Evolve project documentation'
    }
  }
})
