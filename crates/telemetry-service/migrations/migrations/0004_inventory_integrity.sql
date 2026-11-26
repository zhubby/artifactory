-- [INPUT]: 独立清单回执、startup投影与去重正文。
-- [OUTPUT]: 用复合外键阻止仍被新清单引用的正文删除。
-- [POS]: 数据完整性边界；旧清扫失败回滚，不提供混部功能兼容。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
CREATE INDEX inventory_delivery_content ON inventory_deliveries(node_id,inventory_digest);
CREATE INDEX startup_inventory_content ON startup_inventories(node_id,inventory_digest);
ALTER TABLE inventory_deliveries ADD CONSTRAINT inventory_delivery_content
    FOREIGN KEY(node_id,inventory_digest) REFERENCES inventory_versions(node_id,digest)
    ON DELETE RESTRICT;
ALTER TABLE startup_inventories ADD CONSTRAINT startup_inventory_content
    FOREIGN KEY(node_id,inventory_digest) REFERENCES inventory_versions(node_id,digest)
    ON DELETE RESTRICT;
