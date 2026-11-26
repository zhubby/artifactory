# web/src/app/
> L2 | Parent: ../AGENTS.md

成员列表
AGENTS.md: 本目录职责、成员和依赖入口。
apple-icon.png: PNG；apple-icon配置或静态资源。
error.tsx: 通知安全故障文案并保留重试和返回入口。
favicon.ico: 从 icon.svg 导出 16、32、48px 的中性色浏览器回退图标。
globals.css: 加载设备终端、概览、业务详情、审计、下拉与反馈样式，适配系统减少动效。
icon.svg: 深灰底、白色装配 A 浏览器图标，与控制台中性色一致。
layout.tsx: App Router 根布局，建立主题和语言上下文。
manifest.webmanifest: WEBMANIFEST；manifest配置或静态资源。
not-found.tsx: 将未知地址引回已有控制台入口。
page.tsx: 将首页转到租户入口，共用该入口的会话检查。

子目录
docs/: 独立路由或子模块。
join/: 见 join/AGENTS.md。
login/: 见 login/AGENTS.md。
register/: 见 register/AGENTS.md。
tenants/: 见 tenants/AGENTS.md。

notifications/: 提供账户收件箱、显式邮箱验证与退订路由。

[PROTOCOL]: Update this header on change, then check AGENTS.md
