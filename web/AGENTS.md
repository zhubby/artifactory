<!-- BEGIN:nextjs-agent-rules -->

## This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` (resolved from this file's directory; in monorepos the `next` package may not be visible from the repo root) before writing any code. Heed deprecation notices.

This block is written and re-added by `next dev` — verify at `node_modules/next/dist/server/lib/generate-agent-files.js`. Removing it from a diff only re-creates the uncommitted change; committing it with your work keeps the tree clean.

<!-- END:nextjs-agent-rules -->

# web/
> L2 | Parent: ../AGENTS.md

src/: 提供租户控制台、账户收件箱、外部通知配置和公开数据边界。
scripts/: 构建与检查嵌入式文档。
tests/: 验证控制台、签名流程与文档页面。
public/: 保存静态品牌资源及生成的文档。
package.json: 定义 Web 构建与浏览器测试入口。
package-lock.json: 固定前端依赖。
Dockerfile: 构建控制台运行镜像。
next.config.ts: 配置业务代理和文档地址。
next-env.d.ts: 声明 Next.js 环境类型。
tsconfig.json: 配置严格 TypeScript 检查。
postcss.config.mjs: 配置 Tailwind CSS 构建。
components.json: 定义基础控件生成约定。
playwright.config.ts: 配置真实业务浏览器验收。
playwright.ui.config.ts: 配置桌面、平板和手机夹具验收。
playwright.docs.config.ts: 配置匿名文档验收。
THIRD_PARTY_NOTICES.md: 记录第三方组件声明。
.dockerignore: 排除容器构建无关内容。

[PROTOCOL]: Update this header on change, then check AGENTS.md
