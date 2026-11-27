<!--
[INPUT]: xiaojiaenen/liquid-glass 的 README、折射算法与指针高光源码。
[OUTPUT]: 记录参考证据、Prism 吸收的设计原则与实现边界。
[POS]: 光学实验的研究记录；不是外部组件库的安装说明。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 液态玻璃参考研究

参考项目：[xiaojiaenen/liquid-glass](https://github.com/xiaojiaenen/liquid-glass)。
本次读取的仓库首页显示最新提交 `d61ba11`。

[Apple 官方最高准则](APPLE-LIQUID-GLASS.md)约束本研究的使用范围。以下光学设计只补充重要功能层与实验透镜，不能把内容卡片全部改成玻璃。

## 已阅读的证据

| 来源 | 可验证的设计细节 |
| --- | --- |
| [README](https://github.com/xiaojiaenen/liquid-glass/blob/main/README.md) | 区分折射、着色与高光。胶囊、控件、卡片采用不同厚度。Chromium 与其他浏览器采用不同光学路径。 |
| [useGlassParallax.ts](https://github.com/xiaojiaenen/liquid-glass/blob/main/src/lib/useGlassParallax.ts) | 以指针相对中心的 atan2 角度驱动 360° 亮边。通过 rAF 合并更新。 |
| [displacementMap.ts](https://github.com/xiaojiaenen/liquid-glass/blob/main/src/lib/displacementMap.ts) | 从表面高度导出折射轮廓，再按圆角距离场生成 RG 位移贴图。镜面高光为独立贴图。 |
| 首页与目录提交说明 | 独立 BorderGlow 子元素避免与降级伪元素冲突。边框遮罩将反光限定在边缘。 |

GitHub 在读取 `LiquidGlassFilter.tsx` 时返回错误。本次没有据此声称核验该文件内容，也没有安装或运行该仓库。

## Prism 吸收的设计

1. **分离材质层。** 内容、折射与亮边分开。正文不跟随光学层变形。
2. **亮边响应方向。** 高光绕边框转动。指针离开后回到静态材质。
3. **按尺度控制厚度。** 实验的胶囊、控件、大透镜分别使用 10/16/26px 棱镜宽和 38/62/100 虚拟厚度。参数不用于业务内容卡片。
4. **统一弹性节奏。** 按压、胶囊切换、弹层与透镜回弹共享 Prism 的可中断动效规则。
5. **显式降级。** 降低透明度关闭折射与亮边。减少动态效果保留静态玻璃。高对比模式保留实体边界。

适合的 Regular Glass 外壳保留用户指定的 `blur(10px) saturate(210%)`、30px 大外圆角、1px 细弱色散、顶部弧光和内阴影。这些数值是项目参数，不是 Apple 固定标准。

## 本次实现边界

- `motion.js` 与 `liquid.css` 为重要功能外壳和实验透镜增加方向亮边。内容卡片使用稳定表面。
- `optics.js` 独立重写轮廓和距离场，生成本地 RG 位移图。
- 光学实验以同一张本地 SVG 场景同步背景与透镜采样，明确展示边缘折射。
- 折射滤镜只作用于实验透镜的装饰图层。它不抓取任意 DOM 背景，也不扭曲业务文字。
- 拖动有方向键和重置按钮。滑块控制强度。三档预设展示不同棱镜宽度。
- 其他浏览器使用毛玻璃与静态镜面高光。没有常驻扫光循环。

上述实现沿用公开的光学方法与交互原则。Prism 不嵌入该仓库源码，也不新增 React 运行时依赖。参考项目 README 标注 MIT；本交付保留来源链接，未把 README 的许可声明当作对所有依赖的额外核验。
