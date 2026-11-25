-- [INPUT]: 依赖全局账户和现有租户、设备身份。
-- [OUTPUT]: 保存系统级设备公开连接地址。
-- [POS]: 兼容追加迁移；不改写现有节点或凭证。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
CREATE TABLE node_installation_settings (
    singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
    device_endpoint text NOT NULL CHECK(octet_length(device_endpoint)<=320),
    updated_by uuid REFERENCES users(id) ON DELETE SET NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);
