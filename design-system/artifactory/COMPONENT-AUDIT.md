<!--
[INPUT]: 120 项组件源码、Apple 最高准则与浏览器复验。
[OUTPUT]: 记录逐项材质、原问题、设计改动与状态契约。
[POS]: 组件库精修的审查地图；不代表产品实现验收。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 120 项组件逐项审查

此记录对应组件库精修阶段。该阶段保留产品的 Apple 版本。
后续产品复核独立记录于 [PRODUCT-REVIEW.md](PRODUCT-REVIEW.md)。
材质按任务选择，分类不决定玻璃用法。

默认态已逐项查看浅色分类图。深色复验覆盖全部 120 项。
每项均检查渲染、标本宽度和手机溢出。
状态契约列定义所需行为，不等于每项状态均已手工测试。
交互抽查与边界见 [REVIEW.md](REVIEW.md)。

明暗对照：[浅色总览](previews/components-review/light-all.png) · [深色总览](previews/components-review/dark-all.png)。

## 操作与快捷 · 10 项

[浅色](previews/components-review/light-actions.png) · [深色](previews/components-review/dark-actions.png)

| 编号 / 组件 | 材质 | 原问题 | 实际改动 | 状态契约 |
| --- | --- | --- | --- | --- |
| 001 按钮<br>`actions-button` | Prominent / Regular | 操作层级相近，按钮厚重 | 主动作加入启动图标；普通、危险、禁用四层保持44px胶囊轮廓 | 主次危险禁用、按下 |
| 002 图标按钮<br>`actions-icon-button` | Regular | 图标按钮形状和触感不突出 | 48px圆形工具，选中图标有柔和填充与状态描边 | 关注开关、订阅开关、名称 |
| 003 分裂按钮<br>`actions-split-button` | Regular group | 主次按钮圆角割裂 | 共用胶囊外壳、单一分隔、主动作图标与同心内弧 | 展开备选、选择、Escape |
| 004 按钮组<br>`actions-button-group` | Regular group | 小方块缺少连续操作感 | 5px共同外壳内边距、胶囊选中块和40px内部工具 | 三种互斥状态 |
| 005 切换按钮<br>`actions-toggle-button` | Regular | 按钮变化后图标丢失 | 保留心形图标，文字和aria-pressed共同切换 | 开关往返、状态播报 |
| 006 文本链接<br>`actions-link` | Standard | 链接密度紧凑、缺少方向暗示 | 14px文本、18px行间距与触发式箭头位移 | 焦点、下划线、减少动态 |
| 007 浮动操作<br>`actions-fab` | Standard + Prominent | 新增按钮缺少场景 | 工作空间层级排版、30px数量、58px圆形新增按钮 | 新增计数、重复点击 |
| 008 复制按钮<br>`actions-copy-button` | Standard | 长摘要和复制动作层级混合 | 增加SHA-256标签；等宽摘要与局部圆形复制工具 | 复制结果、长摘要折行 |
| 009 加载按钮<br>`actions-loading-button` | Regular | 等待态缺少控制尺寸一致性 | 44px胶囊与玻璃外壳，执行期间禁用重复点击 | 加载禁用与恢复 |
| 010 快捷键<br>`actions-keyboard-shortcut` | Standard | 快捷提示、键帽、动作拥挤 | 独立说明行、实体键帽、明确操作区与手机换行 | Alt+B、按钮、聚焦 |

## 文本与表单 · 10 项

[浅色](previews/components-review/light-forms.png) · [深色](previews/components-review/dark-forms.png)

| 编号 / 组件 | 材质 | 原问题 | 实际改动 | 状态契约 |
| --- | --- | --- | --- | --- |
| 011 文本输入<br>`forms-input` | Standard | 只读和编辑态差异微弱 | 46px实体输入、15px内弧、虚线只读边界 | 输入、只读、focus |
| 012 多行输入<br>`forms-textarea` | Standard | 输入粗细与计数层级模糊 | 正文常规字重、舒适行高和右对齐等宽计数 | 240字上限、计数 |
| 013 密码输入<br>`forms-password` | Standard | 输入和显隐按钮割裂 | 共享16px实体外壳，内嵌38px显隐工具 | 显隐、焦点、值保留 |
| 014 步进数字<br>`forms-number` | Standard | 步进器像三个独立字段 | 共用胶囊、22px等宽数字、圆形两侧操作 | 1–16边界 |
| 015 即时搜索<br>`forms-search` | Standard | 搜索结果文字密度高 | 46px输入与44px列表行，项前小型定位标记 | 匹配、空结果 |
| 016 验证码输入<br>`forms-otp` | Standard | 验证码尺寸偏小 | 56px高六格、23px等宽数码、统一焦点光圈 | 输入、粘贴、退格 |
| 017 单位输入<br>`forms-unit-input` | Standard + Regular | 单位与数值高度不齐 | 稳定单行输入组合与46px单位胶囊 | 单位切换、手机布局 |
| 018 字段验证<br>`forms-validation` | Standard | 错误只依靠细线 | 错误描边与3px低强度外圈、下方修正说明 | 无效到有效、focusout |
| 019 行内编辑<br>`forms-inline-edit` | Standard | 阅读态像禁用字段 | 共享20px阅读外壳，正文实体无框；编辑才显示输入边界 | 编辑保存、值保留 |
| 020 代码输入<br>`forms-code-input` | Standard | 代码与普通输入过于相似 | 18px实体配置面、等宽12px/1.9行高和明确验证动作 | 有效和错误格式 |

## 选择与调节 · 10 项

[浅色](previews/components-review/light-selection.png) · [深色](previews/components-review/dark-selection.png)

| 编号 / 组件 | 材质 | 原问题 | 实际改动 | 状态契约 |
| --- | --- | --- | --- | --- |
| 021 复选框<br>`selection-checkbox` | Standard | 默认原生控件过小 | 20px原生语义复选框、44px选项行、已选浅色面 | 多选与禁用 |
| 022 单选组<br>`selection-radio` | Standard | 互斥层级不明确 | 20px圆形单选、中心圆点与整行选择背景 | 方向键、互斥 |
| 023 开关<br>`selection-switch` | Standard + active glass | 开关像孤立小图标 | 标题/说明双层，52×32轨道与24px拇指，按下触发玻璃 | 即时开关、焦点、减少动态 |
| 024 单选下拉<br>`selection-select` | Regular | 触发器形状普通 | 44px胶囊触发器、箭头开合与临时共享玻璃菜单 | 选择、方向键、Escape |
| 025 可搜索建议<br>`selection-combobox` | Standard + Regular options | 建议列表密度高 | 46px输入、43px建议项、19/13px同心列表与已选勾号 | 筛选、键盘、Escape |
| 026 多选下拉<br>`selection-multi-select` | Regular | 多选摘要和弹层缺少共同规则 | 胶囊摘要配44px选项行，已选状态同时有勾号与背景 | 数量更新、禁用状态 |
| 027 标签输入<br>`selection-tag-input` | Standard | 标签与输入像多层嵌套框 | 统一20px编辑面、胶囊标签、圆形删除动作、无框内部输入 | Enter、重复值、移除 |
| 028 滑块<br>`selection-slider` | Standard + active glass | 原生滑块缺少精细轨道 | 8px胶囊进度轨、28px高光拇指、18px数值；填充随值同步 | 键盘、连续拖动 |
| 029 区间滑块<br>`selection-range-slider` | Standard + active glass | 两个滑块缺少端点层次 | 统一圆端轨道、独立端点标签、等宽区间摘要；越界后两轨同步 | 两端不能交叉 |
| 030 颜色选择<br>`selection-color-picker` | Standard | 色块方圆不一 | 42px圆形色样、高光、独立选中外圈和勾号 | 五种色选择、名称 |

## 日期与文件 · 10 项

[浅色](previews/components-review/light-files.png) · [深色](previews/components-review/dark-files.png)

| 编号 / 组件 | 材质 | 原问题 | 实际改动 | 状态契约 |
| --- | --- | --- | --- | --- |
| 031 日期选择<br>`files-date-picker` | Regular | 日期菜单密度小 | 圆形日期单元、30px弹层壳、清晰月份与圆形翻页工具 | 展开、月份、日期、Escape |
| 032 日期区间<br>`files-date-range` | Regular | 区间摘要层级不足 | 胶囊区间摘要与首尾圆形选择，说明保留独立行 | 开始结束、日期排序 |
| 033 时间选择<br>`files-time-picker` | Regular | 小时与分钟像普通列表 | 两列15px等宽时间、固定说明、胶囊选中态 | 小时、分钟、滚动 |
| 034 日历<br>`files-calendar` | Standard | 日历缺少独立阅读结构 | 最大310px实体内嵌月历、24px外壳、圆形日期和清晰月份 | 月份、选中、手机适配 |
| 035 文件上传<br>`files-upload` | Standard | 上传区只有虚线框 | 54px上传徽记、186px有序落点、14px标题与12px说明 | 点击选择、文件名、焦点 |
| 036 文件列表<br>`files-upload-list` | Standard | 文件名和大小字级过小 | 19px文件行、36×42图标面、12px名称和11px大小 | 增加移除、长文件名 |
| 037 拖放区域<br>`files-dropzone` | Standard | 拖放命中面缺乏反馈 | 208px落点和明确上传徽记；拖入改变边界与图标面 | 拖放、点击、拖出 |
| 038 文件夹选择<br>`files-folder-picker` | Standard | 树状层级拥挤 | 22px实体树面、40px文件夹行、连接线与浅色选择 | 展开、目标摘要 |
| 039 穿梭选择<br>`files-transfer` | Standard | 列标题无实时数量且对象难读 | 18px双列实体面、44px选项、中央圆形动作、实时数量 | 双向移动、数量 |
| 040 级联选择<br>`files-cascader` | Regular | 父层选中态不持续 | 13px同心项轮廓，父层aria-pressed持续显示，完整路径回写 | 父级、叶子、Escape |

## 导航与层级 · 10 项

[浅色](previews/components-review/light-navigation.png) · [深色](previews/components-review/dark-navigation.png)

| 编号 / 组件 | 材质 | 原问题 | 实际改动 | 状态契约 |
| --- | --- | --- | --- | --- |
| 041 面包屑<br>`navigation-breadcrumb` | Standard | 父级路径和当前项权重接近。 | 当前项使用深色半粗体，父级弱化，胶囊热区保留文字路径。 | 点击父级并检查当前位置。 |
| 042 标签导航<br>`navigation-tabs` | Regular + Standard | 轨道与内容框都像普通方块。 | 玻璃胶囊轨道包含同心选中块；正文去嵌套边框，增加平台证据行。 | 方向键切换与面板关联。 |
| 043 分段控制<br>`navigation-segmented` | Regular + Standard | 切换预览只是空框。 | 共享玻璃轨道搭配真实缩略结构，卡片与列表有清晰形态差异。 | 选择列表后检查is-list。 |
| 044 分页<br>`navigation-pagination` | Regular | 每个页码独立描边，缺少一体性。 | 页码并入胶囊玻璃工具组，当前页以凸起实底标注。 | 边界禁用与页码更新。 |
| 045 步骤条<br>`navigation-steps` | Standard | 细线和小圆点缺少进度层次。 | 38px圆节点、双像素连线、当前节点外环与完成态形成顺序。 | 前进与返回并检查完成类。 |
| 046 垂直导航<br>`navigation-vertical-nav` | Regular | 侧栏按钮各自成框。 | 导航项共享26px玻璃外壳；当前项以实体同心曲面抬起。 | 切换后检查aria-current。 |
| 047 资源树<br>`navigation-tree` | Standard | 层级线与选中位置弱。 | 细连接线、42px目录入口、图标分层与左侧强调线标注当前文件。 | 展开assets并选择文件。 |
| 048 页内目录<br>`navigation-toc` | Regular + Standard | 导航与阅读容器缺少分层。 | 页内目录并入胶囊轨道，文档使用20px实体阅读区。 | 点击目录后的区域滚动及焦点。 |
| 049 命令面板<br>`navigation-command-palette` | Regular | 命令像重复普通按钮。 | 菜单使用统一玻璃层，图标、命令、回车提示分列，悬停与焦点强调同一行。 | 过滤与方向键执行。 |
| 050 返回顶部<br>`navigation-back-to-top` | Standard | 返回入口方向不明确。 | 上箭头修正朝向，返回动作置于滚动内容右下侧。 | 滚动后返回顶部。 |

## 数据与资源 · 10 项

[浅色](previews/components-review/light-data.png) · [深色](previews/components-review/dark-data.png)

| 编号 / 组件 | 材质 | 原问题 | 实际改动 | 状态契约 |
| --- | --- | --- | --- | --- |
| 051 基础表格<br>`data-table` | Standard | 11px表格难读，设备与平台无层次。 | 新增设备标题和数量；12px行文、设备图标、平台次级色与CPU微型刻度。 | 检查行列读取与横向滚动。 |
| 052 排序表格<br>`data-sortable-table` | Standard | 排序入口与正文比例失衡。 | 表头保留29px简洁排序入口，图标身份与数字刻度不随排序分离。 | 升序首行为8%。 |
| 053 多选表格<br>`data-selectable-table` | Standard | 选择态只有浅底色。 | 18px复选框和左侧选中线共同表达已选，保留部分全选语义。 | 单行选择后检查indeterminate。 |
| 054 展开行表格<br>`data-expandable-table` | Standard | 详情入口与展开内容拥挤。 | 圆形加号热区、展开态色底、12px证据说明与独立行间距。 | 展开行后核对对应aria-controls。 |
| 055 描述列表<br>`data-description-list` | Standard | 键值排列过密且值缺少权重。 | 15px纵向留白、86px标签列与中等字重值，状态保留文字。 | 长值换行与标签对应。 |
| 056 资源列表<br>`data-list` | Standard | 设备列表图标和文字粘连。 | 图标置于实体13px角方座，名称平台分两层，状态固定右侧。 | 搜索设备与结果数量。 |
| 057 增量列表<br>`data-infinite-list` | Standard | 纯文本记录缺少顺序结构。 | 等宽记录前置两位序号，整宽胶囊加载按钮保持行动入口。 | 加载后新增三行与数量。 |
| 058 头像组<br>`data-avatar-group` | Standard + Regular | 头像组尺寸偏小且溢出成员只有文字。 | 50px重叠身份与独立圆形展开器；玻璃弹层内成员保留头像姓名角色。 | 展开成员弹层。 |
| 059 徽标与计数<br>`data-badge` | Standard | 状态与计数形态普通。 | 语义徽标使用圆润文字胶囊，通知计数独立圆形，状态保留文字。 | 通知计数递减到零。 |
| 060 统计数字<br>`data-statistic` | Standard | 指标字号不突出且周期变化无主次。 | 52px轻字重等宽数字与次级单位，变化趋势放入柔和语义标签。 | 切换7天30天数值。 |

## 图表与进度 · 10 项

[浅色](previews/components-review/light-visualization.png) · [深色](previews/components-review/dark-visualization.png)

| 编号 / 组件 | 材质 | 原问题 | 实际改动 | 状态契约 |
| --- | --- | --- | --- | --- |
| 061 折线图<br>`visualization-line-chart` | Standard | 趋势与精确指标缺少层级。 | 新增平均耗时抬头，留白分隔柔和曲线与时间范围；保留保形曲线。 | 切换30天平均值7.6。 |
| 062 柱状图<br>`visualization-bar-chart` | Standard | 图形总量与每日值缺少关联。 | 新增总量抬头，数值与刻度字号提升，保持上下相同胶囊圆角。 | 切换30天总量391。 |
| 063 环形图<br>`visualization-donut-chart` | Standard | 中心数字与图例节奏不足。 | 35px中心量、17px图例间隔与8px语义点，保留真实比例和微小段圆端。 | 切换小比重与统计窗口。 |
| 064 矩阵热力图<br>`visualization-heatmap` | Standard | 采样说明弱且方格过机械。 | 新增采样总数抬头，7px圆角格、6px间距和稳定实体底。 | 检查文字范围与色阶。 |
| 065 迷你趋势<br>`visualization-sparkline` | Standard | 指标和微图比例不协调。 | 26px精确耗时与紧凑趋势并排，微图填充弱化以保留主指标。 | 检查缩小后数值仍可读。 |
| 066 线性进度<br>`visualization-progress` | Standard | 进度轨道和百分比权重不足。 | 12px圆轨保留柔和渐变，25px数字与任务标签两端对齐。 | 推进到100和0。 |
| 067 环形进度<br>`visualization-progress-ring` | Standard | 中心比例字号偏弱。 | 34px轻字重中心值与11px说明形成层级，保留圆端单轨。 | 推进到83、100、0。 |
| 068 容量仪表<br>`visualization-gauge` | Standard | 容量值与资源单位难区分。 | 40px已用槽位配次级上限，说明11px，弧线保留圆端和渐变。 | 新增任务后meter数值同步。 |
| 069 时间轴<br>`visualization-timeline` | Standard | 事件点都同样强调。 | 首个事件实心并加外环，历史事件空心，时间与事件正文分列。 | 追加最早源码事件。 |
| 070 日历热力图<br>`visualization-calendar-heatmap` | Standard | 月份标题和采样点同权。 | 31px月份与次级窗口标签，6px圆角格按日期排列。 | 切换上一窗口。 |

## 内容与媒体 · 10 项

[浅色](previews/components-review/light-content.png) · [深色](previews/components-review/dark-content.png)

| 编号 / 组件 | 材质 | 原问题 | 实际改动 | 状态契约 |
| --- | --- | --- | --- | --- |
| 071 手风琴<br>`content-accordion` | Standard | 标题热区和展开层次弱。 | 54px标题行、圆形展开符与14px标题，已展开标题使用强调色。 | 同组details互斥展开。 |
| 072 渐进披露<br>`content-disclosure` | Standard | 高级参数像普通细边框。 | 19px同心阅读容器配17px内距，展开后标签和值保留一致列宽。 | 展开高级参数。 |
| 073 局部滚动区<br>`content-scroll-area` | Standard | 阅读区内距少且字号小。 | 20px阅读圆角与19px内距，13px正文及1.9行高。 | 键盘滚动与焦点边界。 |
| 074 可调分栏<br>`content-split-pane` | Standard | 分隔条只是字符，抓取意图不清。 | 19px实体抓取带、29px圆形握柄，预览和源码均有17px内距。 | 拖动、方向键和range共享百分比。 |
| 075 手动轮播<br>`content-carousel` | Standard + Clear | 导航脱离媒体且作品比例弱。 | 226px作品画面配悬浮Clear胶囊工具，标题使用实体白底。 | 下一张后索引2/3。 |
| 076 图片预览<br>`content-image-preview` | Standard + Clear | 放大入口独立在图片下方。 | 将放大入口置于作品右下角Clear玻璃胶囊，窗口圆角随外壳递减。 | 打开预览、Escape关闭、焦点返回。 |
| 077 代码块<br>`content-code-block` | Standard | 代码标题和复制入口僵硬。 | 标题增加语言状态点，17px同心外形、12px代码与1.95行高。 | 复制反馈或系统复制降级。 |
| 078 差异查看<br>`content-diff-view` | Standard | 差异层次和行块过密。 | 17px外壳、15px段落内距，增删背景与符号双编码，手机并排降为堆叠。 | 切换并排与统一视图。 |
| 079 富文本预览<br>`content-markdown` | Standard | 文档字号与标题缺少阅读层级。 | 21px标题、13px正文、分隔线与圆角引用块构成文档节奏。 | 切换源文和排版预览。 |
| 080 空状态<br>`content-empty-state` | Standard + Prominent | 空状态图标与动作缺少焦点。 | 57px柔和图标座与17px标题，单一强调胶囊创建动作。 | 创建示例并恢复空状态。 |

## 反馈与状态 · 10 项

[浅色](previews/components-review/light-feedback.png) · [深色](previews/components-review/dark-feedback.png)

| 编号 / 组件 | 材质 | 原问题 | 实际改动 | 状态契约 |
| --- | --- | --- | --- | --- |
| 081 提示条<br>`feedback-alert` | Standard | 通栏色块与小字缺少主次 | 左侧语义线、18px间距和标题正文分层 | 收起与恢复 |
| 082 顶部通知<br>`feedback-notification` | Regular Glass | 通知缺少浮起材质与动作主次 | 顶部胶囊Regular外壳、状态图标与独立关闭 | 成功/失败/关闭 |
| 083 消息横幅<br>`feedback-message-banner` | Standard | 横幅仅依赖底色区分 | 修订编号、18px标题、24px圆角与独立下一步 | 展开变化 |
| 084 加载旋转器<br>`feedback-spinner` | Standard | 加载行缺少稳定占位 | 58px内容槽、24px旋转器与禁用主动作 | 开始/完成/重复点击 |
| 085 内容骨架<br>`feedback-skeleton` | Standard | 骨架密度和最终内容缺少对应 | 42px头像与14px标题占位、24px内容面 | 骨架/就绪 |
| 086 局部加载层<br>`feedback-loading-overlay` | Standard | 局部任务范围与层级不清 | 项目资源眉题、实体内容面和不位移取消层 | 刷新/取消/完成 |
| 087 结果页<br>`feedback-result` | Standard | 结果图标与文案比例单薄 | 68px语义章、21px标题、限制正文宽度 | 成功/失败 |
| 088 可恢复失败<br>`feedback-error-state` | Standard | 失败区整面染色挤压恢复动作 | 中性对象面、局部危险图标、主要重试动作 | 失败/恢复图标文字 |
| 089 离线提示<br>`feedback-offline-banner` | Standard | 离线信息像普通备注 | 语义边线与快照时间分层，恢复为主要动作 | 离线/重连 |
| 090 可撤销操作<br>`feedback-undo` | Standard | 标签与撤销反馈缺少对象边界 | 标签胶囊、项目标签眉题、独立撤销条 | 移除/撤销焦点 |

## 浮层与引导 · 10 项

[浅色](previews/components-review/light-overlays.png) · [深色](previews/components-review/dark-overlays.png)

| 编号 / 组件 | 材质 | 原问题 | 实际改动 | 状态契约 |
| --- | --- | --- | --- | --- |
| 091 工具提示<br>`overlays-tooltip` | Standard | 说明气泡过窄且拥挤 | 260px阅读宽度、20px圆角、15px内边距 | 悬停/Tab读取 |
| 092 信息浮层<br>`overlays-popover` | Regular Glass | 偏好内容与菜单项层次相同 | 24px共享玻璃壳、标题/选项/主要应用动作 | 勾选/应用/Escape |
| 093 操作菜单<br>`overlays-dropdown` | Regular Glass | 危险项未在视觉分隔 | 46px胶囊触发器、44px菜单项、末项危险分隔 | 展开/方向键/选择 |
| 094 上下文菜单<br>`overlays-context-menu` | Regular Glass | 文件目标像无差别虚线框 | DMG文件章、标准对象面与同源菜单 | 右键/ShiftF10/可见按钮 |
| 095 模态对话框<br>`overlays-dialog` | Regular Glass + Standard | 整个对话框材质与正文混同 | 30px玻璃任务壳、20px实体阅读内面与主动作 | 打开/Escape/焦点返回 |
| 096 侧边抽屉<br>`overlays-drawer` | Regular Glass + Standard | 侧栏只是拉长的弹窗 | 全高玻璃外壳、实体滚动正文与独立关闭 | 打开/滚动/关闭 |
| 097 危险操作确认<br>`overlays-confirm-dialog` | Regular Glass + Standard | 最终危险动作缺少明确层级 | 实体确认字段、语义危险主按钮与返回 | ARCHIVE解锁/Escape |
| 098 底部面板<br>`overlays-sheet` | Regular Glass + Standard | 底部入口缺少手柄和选中层级 | 同心面板、居中手柄、分组目标与选中底 | 多选/应用/返回 |
| 099 引导气泡<br>`overlays-coachmark` | Regular Glass | 步骤文字与行动缺少节奏 | 眉题步骤、17px标题、主要下一步和次要跳过 | 三步/跳过/Escape |
| 100 聚焦预览<br>`overlays-lightbox` | Regular + Standard / media | 媒体概念图尺度不足 | 280px媒体环境与160px光学透镜，关闭位固定 | 打开/关闭 |

## 构建与交付 · 10 项

[浅色](previews/components-review/light-delivery.png) · [深色](previews/components-review/dark-delivery.png)

| 编号 / 组件 | 材质 | 原问题 | 实际改动 | 状态契约 |
| --- | --- | --- | --- | --- |
| 101 构建阶段<br>`delivery-build-stage` | Standard | 阶段间缺乏连接与当前焦点 | 连续细线、39px圆形节点与当前语义环 | 推进/完成/重置 |
| 102 目标矩阵<br>`delivery-target-matrix` | Standard | 表格小字拥挤且选择不突出 | 13px正文、14px单元留白、选中行淡色 | 勾选计数 |
| 103 日志查看器<br>`delivery-log-viewer` | Standard | 日志缺少任务身份与阅读留白 | 构建编号栏、12px等宽日志和22px实体控制台 | 过滤/行数 |
| 104 终端会话<br>`delivery-terminal` | Standard | 终端缺少会话范围的固定标识 | 本地模拟标题、无连接标识与32px命令输入 | help/pwd/uname/clear |
| 105 制品卡<br>`delivery-artifact-card` | Standard | 制品名与证据按钮缺少主对象层级 | 50px文件章、16px名称、平台元数据与证据展开 | 展开证据 |
| 106 发布门禁<br>`delivery-release-gate` | Standard | 确认项像松散按钮集合 | 连续分组清单、选中底色与冻结动作隔离 | 三项解锁/冻结 |
| 107 排队任务<br>`delivery-queue-card` | Standard | 队列序号未成为主要线索 | 42px序号、竖线分隔、平台与原因两层 | 优先/恢复排序 |
| 108 依赖图<br>`delivery-dependency-graph` | Standard | 节点过小且选中轮廓单薄 | 88px节点、22px圆角、选择环和独立连接 | 逐节点选择/详情 |
| 109 审批卡<br>`delivery-approval-card` | Standard | 申请与批准拒绝按钮同权重 | 项目范围眉题、50px身份头像与决策主次 | 批准/拒绝 |
| 110 摘要复制<br>`delivery-digest-copy` | Standard | 摘要字段与按钮密度不均 | 身份眉题、46px等宽字段与明确复制主动作 | 复制/回退选择 |

## 组织与资源 · 10 项

[浅色](previews/components-review/light-organization.png) · [深色](previews/components-review/dark-organization.png)

| 编号 / 组件 | 材质 | 原问题 | 实际改动 | 状态契约 |
| --- | --- | --- | --- | --- |
| 111 成员卡<br>`organization-member-card` | Standard + Regular Glass | 成员身份与范围入口缺乏分层 | 50px圆头像、16px姓名和共享玻璃范围菜单 | 范围展开/选择 |
| 112 权限矩阵<br>`organization-permission-matrix` | Standard | 权限表小字与命中区偏小 | 13px表文、18px勾选与完整行高 | 切换权限/计数 |
| 113 凭据引用<br>`organization-credential-reference` | Standard | 凭据引用像普通文件名 | 50px钥匙章、授权元数据与检查主动作 | 检查/展开详情 |
| 114 设备资源卡<br>`organization-device-card` | Standard | 设备状态与容量层级不足 | 16px设备名、平台槽位分层和柔和渐变容量 | 在线/离线 |
| 115 资源仪表<br>`organization-resource-meter` | Standard | 数值与普通正文等大 | 24px精确读数、资源眉题与圆端渐变仪表 | 轻负载/高负载 |
| 116 审计事件<br>`organization-audit-event` | Standard | 事件时间与主体缺少阅读秩序 | 身份图章、15px事件标题、时间对象次行 | 变更前后展开 |
| 117 活动流<br>`organization-activity-feed` | Standard | 时间线密度与筛选状态不清 | 连续线与带留白节点、胶囊筛选和选中态 | 全部/仅构建 |
| 118 配额卡<br>`organization-quota-card` | Standard | 配额数字未突出且临界状态不明显 | 24px读数、圆端渐变与接近上限警示色 | 72/96切换 |
| 119 通知条目<br>`organization-notification-item` | Standard | 通知未读点占据正文空间 | 独立圆角收件项、右上未读点和对象图章 | 已读/未读 |
| 120 工作空间切换<br>`organization-workspace-switcher` | Regular Glass | 空间切换入口与普通菜单无差别 | 58px身份触发器、21px同心轮廓与角色菜单 | 空间切换/焦点返回 |

## 验证边界

- 120 个唯一 ID，12 类各 10 项，无空标本。
- 全展厅明暗主题默认 Axe 均为零违规。
- 1440px 桌面、375px 手机与812×375横屏无整页溢出。
- 200% 文字放大模拟无整页溢出。
- 五类任务弹层通过键盘关闭与焦点恢复。
- 输入23项状态、数据导航核心操作、反馈16项行为已抽查。
- 小比重环段、等圆角柱体与图表更新边界已复验。
- 动效运行中切换减少动态后动画清零。
- 降低透明度、增强对比分别关闭背景滤镜。
- 三个产品页面禁用全部组件库样式前后截图一致。
- 12 个受保护产品文件的摘要与回滚基线一致。

自动检查不能替代真实屏幕阅读器与实体触屏验收。
本轮实际浏览器为 Chromium。Safari 与 Firefox 未验收。
