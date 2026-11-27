/**
 * [INPUT]: 读取浏览器演示状态，不请求设备或控制平面。
 * [OUTPUT]: 提供设备快照、节点池字段与名称校验。
 * [POS]: 设备原型的数据边界；保留未知、零和布尔值。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export const platformVersion = '0.2.5';
const snapshots = [
    { key: 'studio', name: 'Mac Studio', hostname: 'orbital-studio.local', id: 'a7e4b912-64f0-4e37-a125-6e0d0bc88f42', os: 'macOS 15.2', arch: 'arm64', chip: 'Apple M2 Ultra', cores: 24, memory: 192, slots: 4, active: 1, online: true, glyph: 'apple', pool: 'apple', version: '0.2.4', tags: ['macOS', 'iOS', '签名'], observed: '2026-11-26 14:30:12 +08:00' },
    { key: 'windows', name: 'Windows Builder', hostname: 'win-builder-01', id: 'c9f2d043-3f16-4ff2-a9de-e25a4ce09a43', os: 'Windows 11', arch: 'amd64', chip: 'AMD Ryzen 9 7950X', cores: 16, memory: 64, slots: 2, active: 0, online: true, glyph: 'windows', pool: 'windows', version: '0.2.5', tags: ['Windows', '代码签名'], observed: '2026-11-26 14:30:08 +08:00' },
    { key: 'linux', name: 'Linux Runner', hostname: 'linux-runner-01', id: 'e1b8a650-b5c2-4ddf-b074-08e83359dadb', os: 'Ubuntu 24.04', arch: 'amd64', chip: 'AMD EPYC 7543', cores: 32, memory: 128, slots: 8, active: 0, online: true, glyph: 'linux', pool: 'linux', version: '0.2.5', tags: ['Linux', 'Android'], observed: '2026-11-26 14:30:10 +08:00' },
    { key: 'mini', name: 'Mac mini', hostname: 'mini-lab.local', id: 'f0c7e328-63b8-4a21-b834-06ea70815e24', os: 'macOS 15.1', arch: 'arm64', chip: 'Apple M4', cores: 10, memory: 24, slots: 2, active: null, online: false, glyph: 'apple', pool: 'apple', version: null, tags: ['macOS', '实验室'], observed: null }
];
export function getDevices(state = {}) {
    return snapshots.map(device => ({ ...device, name: state.deviceNames?.[device.key] ?? device.name, version: state.deviceVersions?.[device.key] ?? device.version }));
}
export const getDevice = (state, key = state.nodeId || 'studio') => getDevices(state).find(device => device.key === key);
export function getPools(state) {
    state.demoPools ||= [
        { id: 'apple', name: 'Apple 原生节点池', is_default: true, node_count: 2, online_count: 1 },
        { id: 'windows', name: 'Windows 原生节点池', is_default: false, node_count: 1, online_count: 1 },
        { id: 'linux', name: 'Linux 原生节点池', is_default: false, node_count: 1, online_count: 1 },
        { id: 'reserve', name: '预备节点池', is_default: false, node_count: 0, online_count: 0 },
        { id: 'unreported', name: '未汇总节点池', is_default: false, node_count: null, online_count: null }
    ];
    // 旧版本地快照没有默认标记；只补已知默认身份，缺失计数仍为未知。
    state.demoPools = state.demoPools.map(pool => pool.is_default == null ? { ...pool, is_default: pool.id === 'apple' } : pool);
    return state.demoPools;
}
export const known = (value, suffix = '') => value == null ? '未知' : `${value}${suffix}`;
export function validateDeviceName(raw) {
    const name = String(raw ?? '').replace(/^\p{White_Space}+|\p{White_Space}+$/gu, '');
    const bytes = new TextEncoder().encode(name).length;
    const error = !name ? '请输入名称。' : /\p{Cc}/u.test(name) ? '名称不能包含控制字符。' : bytes > 100 ? `名称为 ${bytes} 个 UTF-8 字节，请控制在 100 字节内。` : '';
    return { name, bytes, error };
}
