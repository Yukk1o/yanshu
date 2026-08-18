# 维护 Wiki

Wiki 使用成熟的开源静态文档框架 [VitePress](https://vitepress.dev/)，采用官方默认主题、官方本地搜索和少量项目 CSS。没有第三方主题，也没有自造 Wiki 引擎。

## 环境要求

- Node.js 18、20 或 22+（推荐当前 LTS）；
- npm；
- 在仓库的 `wiki/` 目录运行命令。

当前锁定稳定版 VitePress 1.6.4。依赖、cache 和构建结果都留在 `wiki/`，与 Rust workspace 隔离。

## 安装与本地开发

```powershell
Set-Location wiki
npm install
npm run dev
```

打开终端显示的地址，默认通常为 <http://127.0.0.1:5173/>。开发服务支持 Markdown 热更新。

## 生产构建

```powershell
Set-Location wiki
npm run build
npm run preview
```

构建结果位于 `wiki/.vitepress/dist/`。`preview` 只用于本地检查产物，不是生产服务器。

以上用法遵循 VitePress 官方的[安装指南](https://vitepress.dev/guide/getting-started)和[部署指南](https://vitepress.dev/guide/deploy)。

## 站内搜索

配置使用 VitePress 内置的浏览器端模糊全文搜索：

```ts
themeConfig: {
  search: { provider: 'local' }
}
```

不需要 Algolia 账号或第三方搜索插件。配置依据：[VitePress Search](https://vitepress.dev/reference/default-theme-search)。

## 为什么构建前会同步源码

`npm run dev` 和 `npm run build` 的 pre-script 会先执行：

```powershell
npm run sync-source
```

`wiki/scripts/sync-source.mjs` 只复制白名单文件到 `wiki/public/source/`，让网页中的“真实源码”链接可用。这个目录是生成物，不提交 Git；每次构建都会用当前工作树内容覆盖对应文件。

白名单是安全边界。新增链接时应显式加入单个文件，不要把整个仓库递归复制，尤其不要发布：

- `.env` 或密钥；
- `.runtime/` 数据与 observations；
- `.git/`；
- npm cache / node_modules。

## 内容与导航

- 页面使用普通 Markdown；
- 导航和 sidebar 在 `.vitepress/config.mts`；
- 主题只扩展官方 default theme，入口在 `.vitepress/theme/index.ts`；
- 项目色彩和排版在 `.vitepress/theme/custom.css`；
- 本地搜索索引在构建时由 VitePress 生成。

新增页面后要同时把它加入 sidebar，保证移动端目录也能直接找到。

## 提交前检查

```powershell
Set-Location wiki
npm run build
git status --short
```

确认 build 成功，并且没有 `node_modules/`、`.npm-cache/`、`.vitepress/dist/` 或 `public/source/` 出现在 Git 待提交列表。
