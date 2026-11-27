<!--
[INPUT]: Apple Liquid Glass 技术概览、采用指南与 HIG 材质和颜色指南。
[OUTPUT]: 提供最高设计准则、证据来源和 Prism 的 Web 解释。
[POS]: Prism 规范的优先级入口；MASTER 与组件分册据此定义细节。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Apple Liquid Glass · 最高设计准则

研读日期：2026-11-27。

Apple 官方设计原则高于 Prism 的早期材质方案。
第三方研究用于光学实验，不能覆盖官方原则。
业务权限、数据真实性与可访问性要求继续生效。

## 官方来源

| 来源 | 本次读取范围 |
| --- | --- |
| [Liquid Glass 技术概览](https://developer.apple.com/cn/documentation/technologyoverviews/liquid-glass) | 设计原则、导航、颜色与跨设备一致性 |
| [Adopting Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass) | Visual refresh、Controls、Navigation、Menus and toolbars、Windows and modals、Organization and layout |
| [HIG · Materials](https://developer.apple.com/design/human-interface-guidelines/materials#Liquid-Glass) | 功能层、内容层、regular 与 clear、辅助偏好 |
| [HIG · Color](https://developer.apple.com/design/human-interface-guidelines/color#Liquid-Glass-color) | 无固有色、单色工具、主要动作与材质自适应 |

正文通过 Apple 官方 DocC JSON 读取。
原生 API 的自动行为不等于 Web 自动支持。
以下表格区分官方原则与本项目解释。

## 原则与落点

| 官方原则 | Prism 的 Web 解释 |
| --- | --- |
| Liquid Glass 是独立的功能层，浮于内容之上。 | 侧栏、导航、工具组和浮层使用玻璃。 |
| 内容层使用 standard materials。 | 指标、项目、设备、表格和日志采用稳定底色。 |
| 自定义玻璃控件应克制，聚焦重要功能。 | 组件标本外框不加玻璃。行内动作使用普通按钮。 |
| regular 模糊并调节亮度，保持文字清晰。 | 业务导航、弹出菜单和文字浮层默认 regular。 |
| clear 适合浮于照片或视频的控件。 | clear 只在媒体或光学实验使用，不作数据底板。 |
| clear 在明亮内容上需要适当压暗。 | 媒体控件按合成背景检查；必要时加入 35% 黑色遮罩。 |
| 避免拥挤和玻璃层层堆叠。 | 同类工具共享外壳，内部按钮不各自重复模糊。 |
| 工具项按功能分组，并保持位置一致。 | 搜索独立，账户工具成组；页面动作仍在标题区。 |
| 硬件与容器曲率形成同心形状。 | 内圆角依容器半径与间距递减，避免随机圆角。 |
| 颜色用于真正需要强调的动作和状态。 | 工具与导航以中性材质为主，每页保留单一主动作。 |
| 材质响应交互，滑块和开关在激活时呈现玻璃。 | 拖动、按下与选择触发局部光学反馈，空闲保持静止。 |
| 液态形变连接按钮、菜单和浮层。 | 动效从触发位置连续展开，文字保持原形。 |
| 滚动边缘效果维持控件的对比。 | 固定顶栏增加柔和渐隐，焦点不被遮挡。 |
| 全高 sheet 更不透明，帮助聚焦任务。 | 手机全屏编辑与长表单采用实体阅读底。 |
| 列表、表格与表单增加留白，改善阅读。 | 行高、字段组距与内容宽度按阅读任务统一。 |
| 尊重减少透明度、减少动态和增加对比。 | 独立响应各项偏好，保留可识别边界和焦点。 |

## 统一材质词典

- **Canvas**：页面背景与低强度静态环境色。
- **Standard**：内容表面，不使用液态反光与折射。
- **Regular Glass**：导航、重要工具组与临时浮层。
- **Clear Glass**：媒体上方的少量控件与光学实验。
- **Prominent**：唯一主要动作，使用强调色背景。

不再用「Clear」指代不透明阅读面板。
透明程度不能代替信息层级。
玻璃边缘不能成为对象状态的唯一标记。

## 光学参数的身份

`blur(10px) saturate(210%)`、30px 外圆角、
1px 色散边、顶部弧光与深柔阴影，
均为 Prism 的 Web 表达与用户指定风格。
这些值不是 Apple 发布的固定参数。

参数只应用于适合的 Regular Glass 外壳。
小控件按自身尺寸使用胶囊或同心圆角。
色散保持细弱，不把导航染成彩虹。
不通过提高模糊强度弥补文字对比不足。

第三方的 Snell 位移、10/16/26px 棱镜宽度，
以及 38/62/100 虚拟厚度属于光学实验。
实验只折射本地图像层，业务正文不位移。
Web 原型不宣称复现 Apple 原生光学渲染。

## 评审顺序

1. 先确认用户正在阅读或操作的核心对象。
2. 再划分内容层、导航层和临时任务层。
3. 为重要功能选择 Regular Glass。
4. 合并相同任务的工具，检查内外圆角。
5. 检查明暗、滚动、缩放和辅助偏好。
6. 最后调整高光、色散与触发式动效。

界面空闲时，内容与操作位置保持稳定。
高光服务于层级，动画服务于空间连续性。
