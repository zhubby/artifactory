/**
 * [INPUT]: 依赖 PRODUCT.md 的项目、构建与交付场景。
 * [OUTPUT]: 导出演示项目、构建、通知与导航索引。
 * [POS]: 仅供静态原型；所有数据均为虚构样例。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export const nav = [
    { group: '工作空间', items: [['overview', 'grid', '概览'], ['projects', 'folder', '项目'], ['builds', 'play', '构建', 3], ['releases', 'layers', '版本与交付'], ['artifacts', 'box', '制品库'], ['registry', 'cube', '镜像仓库']] },
    { group: '构建基础设施', items: [['nodes', 'monitor', '构建设备'], ['pools', 'server', '构建池'], ['observability', 'chart', '观测']] },
    { group: '工作空间管理', items: [['credentials', 'key', '凭据与签名'], ['team', 'users', '团队与权限'], ['activity', 'shield', '审计记录'], ['settings', 'settings', '设置']] }
];
export const projects = [
    { id: 'orbital', name: 'Orbital Desktop', initial: 'O', color: 'blue', description: '让想法在每一块屏幕上自由流动。', type: '跨平台桌面应用', repo: 'orbital/desktop', version: '2.4.0', status: '构建中', kind: 'running', platforms: ['macOS', 'Windows', 'Linux', 'OCI'], builds: 128, success: '98.4%', last: '2 分钟前' },
    { id: 'atlas', name: 'Atlas API', initial: 'A', color: 'purple', description: '连接产品与数据的可靠服务层。', type: '容器化后端服务', repo: 'orbital/atlas-api', version: '1.8.2', status: '已发布', kind: 'success', platforms: ['OCI amd64', 'OCI arm64'], builds: 86, success: '99.2%', last: '24 分钟前' },
    { id: 'beacon', name: 'Beacon Mobile', initial: 'B', color: 'apricot', description: '为移动中的团队传递每一个重要信号。', type: '移动应用', repo: 'orbital/beacon', version: '0.9.0-rc.1', status: '待发布', kind: 'warning', platforms: ['iOS', 'Android'], builds: 42, success: '96.8%', last: '1 小时前' },
    { id: 'docs', name: 'Design Portal', initial: 'D', color: 'green', description: '设计、文档与团队知识，在同一个地方。', type: 'Web 应用', repo: 'orbital/design-portal', version: '1.2.0', status: '已发布', kind: 'success', platforms: ['OCI amd64'], builds: 31, success: '100%', last: '3 小时前' }
];
export const builds = [
    ['#1048', 'Orbital Desktop', '2.4.0', 'release/2.4', 'e7a2c91', '构建中', 'running', '4 / 6', '08:42', '林知远'],
    ['#1047', 'Orbital Desktop', '2.4.0', 'release/2.4', 'e7a2c91', '成功', 'success', '6 / 6', '11:24', '林知远'],
    ['#1046', 'Atlas API', '1.8.2', 'release/1.8', 'b42e8af', '成功', 'success', '2 / 2', '02:31', '陈雨'],
    ['#1045', 'Beacon Mobile', '0.9.0-rc.1', 'develop', 'f39c0b1', '等待设备', 'warning', '0 / 2', '排队中', '周可'],
    ['#1044', 'Orbital Desktop', '2.3.9', 'main', 'de42c80', '失败', 'danger', '4 / 6', '03:09', '林知远']
];
export const notices = [
    { title: 'Orbital Desktop · 2.4.0 等待交付', body: '构建 #1047 的全部目标已完成。查看签名和交付清单。', time: '8 分钟前', icon: 'layers', route: 'release', kind: 'success' },
    { title: 'macOS 签名证书将在 30 天后到期', body: 'Developer ID · Orbital Labs，建议提前轮换凭据。', time: '26 分钟前', icon: 'key', route: 'credentials', kind: 'warning' },
    { title: 'Atlas API · 1.8.2 已发布', body: '两个镜像架构已交付，摘要校验完成。', time: '32 分钟前', icon: 'check', route: 'registry', kind: 'success' },
    { title: 'Mac mini 已离线', body: 'Beacon Mobile 构建 #1045 仍在排队。设备恢复后继续。', time: '1 小时前', icon: 'monitor', route: 'nodes', kind: 'warning' }
];
