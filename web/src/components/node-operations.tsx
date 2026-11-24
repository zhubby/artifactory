/**
 * [INPUT]: 依赖节点授权、终端路由、OTA API 与节点展示。
 * [OUTPUT]: 提供终端入口、升级、配置、诊断和吊销。
 * [POS]: 设备详情的运维分区；保留授权校验、轮询与嵌套弹窗流程。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useRef, useState } from "react";
import { Settings2, Terminal } from "lucide-react";
import { ErrorBox } from "@/components/common";
import { NodeOTA } from "@/components/emissary-ota";
import { NodeForm } from "@/components/forms";
import {
  NodeDataError,
  NodeDisclosure,
  NodeFacts,
  NodeRawData,
  NodeSection,
} from "@/components/node-detail-parts";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Modal } from "@/components/ui/dialog";
import { mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import type { EmissaryRelease, EmissaryUpdate, Node, Pool, User } from "@/lib/types";
import { bytes } from "@/lib/utils";
import { Link } from "@/lib/tenant";

export function NodeOperations({
  node,
  user,
  pools,
  reload,
}: {
  node: Node;
  user: User;
  pools: Pool[];
  reload: () => Promise<unknown>;
}) {
  const { t } = useUI();
  const [editing, setEditing] = useState(false);
  const [revoking, setRevoking] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error>();
  const content = useRef<HTMLDivElement>(null);
  const releases = useRemote<EmissaryRelease[]>("/emissary-releases", 30000);
  const updates = useRemote<EmissaryUpdate[]>("/emissary-updates", 3000);
  async function refresh() {
    await Promise.all([reload(), releases.reload(), updates.reload()]);
  }
  async function revoke() {
    setBusy(true);
    setError(undefined);
    try {
      await mutation(
        `/nodes/${node.id}`,
        {
          draining: true,
          revoked: true,
          slots: node.slots,
          pool_id: node.pool_id,
          labels: node.labels,
          signing_profiles: node.signing_profiles,
        },
        "PATCH",
      );
      setRevoking(false);
      notify.success(t("common.operationSucceeded"));
      await refresh();
      content.current?.closest("[data-app-modal]")
        ?.querySelector<HTMLButtonElement>('[role="tab"][aria-selected="true"]')?.focus();
    } catch (cause) {
      setError(asError(cause));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="node-section-stack" ref={content}>
      {user.role === "admin" && <NodeSection title={t("terminal.title")}>
        <p className="mb-4 text-sm leading-6 text-muted-foreground">
          {!node.capabilities.includes("terminal-v1") ? t("terminal.disabled")
            : !node.online ? t("terminal.offline") : t("terminal.entryHint")}
        </p>
        {node.online && !node.revoked && !node.draining && !node.ota_draining && node.capabilities.includes("terminal-v1") ? (
          <Button variant="secondary" asChild><Link href={`/nodes/${encodeURIComponent(node.id)}/terminal`}>
            <Terminal size={15} />{t("terminal.open")}
          </Link></Button>
        ) : <Button variant="secondary" disabled><Terminal size={15} />{t("terminal.open")}</Button>}
      </NodeSection>}
      <NodeSection title={t("ota.title")}>
        <NodeDataError error={releases.error || updates.error} retry={refresh} />
        <NodeOTA
          node={node}
          user={user}
          releases={releases.data || []}
          updates={updates.data || []}
          reload={refresh}
          actionsReady={
            !!releases.data &&
            !!updates.data &&
            !releases.error &&
            !updates.error
          }
        />
      </NodeSection>
      {user.role === "admin" && (
        <NodeSection title={t("nodes.configure")}>
          {editing ? (
            <NodeForm
              node={node}
              pools={pools}
              done={() => {
                setEditing(false);
                void refresh();
              }}
            />
          ) : (
            <>
              <p className="mb-4 text-sm leading-6 text-muted-foreground">
                {t("nodes.configureHint")}
              </p>
              <Button variant="secondary" onClick={() => setEditing(true)}>
                <Settings2 size={15} />
                {t("common.settings")}
              </Button>
            </>
          )}
        </NodeSection>
      )}
      <NodeDisclosure title={t("nodes.runtime")}>
        <NodeFacts
          entries={[
            [
              t("ota.current"),
              node.emissary_current_version || node.diagnostics.emissary_version,
            ],
            [t("nodes.emissaryCommit"), node.diagnostics.emissary_commit],
            [t("nodes.protocol"), node.diagnostics.protocol_major],
            [
              t("nodes.processMemory"),
              node.diagnostics.metrics?.emissary_rss_bytes == null
                ? t("nodes.unknown")
                : bytes(node.diagnostics.metrics.emissary_rss_bytes),
            ],
            [
              t("nodes.logBuffer"),
              node.diagnostics.buffered_log_bytes == null
                ? t("nodes.unknown")
                : bytes(node.diagnostics.buffered_log_bytes),
            ],
          ]}
        />
        <NodeRawData value={node.diagnostics} label={t("nodes.fullDiagnostics")} />
        {node.emissary_ota_status && (
          <NodeRawData value={node.emissary_ota_status} label={t("nodes.fullOtaStatus")} />
        )}
      </NodeDisclosure>
      {user.role === "admin" && !node.revoked && (
        <div className="node-danger-zone">
          <div>
            <h3>{t("nodes.revokeTitle")}</h3>
            <p>{t("nodes.revokeDescription", { name: node.name })}</p>
          </div>
          <Button
            variant="destructive"
            size="sm"
            onClick={() => {
              setError(undefined);
              setRevoking(true);
            }}
          >
            {t("common.revoke")}
          </Button>
        </div>
      )}
      <Modal
        open={revoking}
        onOpenChange={(open) => {
          if (!busy) setRevoking(open);
        }}
        title={t("nodes.revokeTitle")}
        description={t("nodes.revokeDescription", { name: node.name })}
      >
        <ErrorBox message={error} />
        <div className="form-footer">
          <Button variant="secondary" disabled={busy} onClick={() => setRevoking(false)}>
            {t("nodes.keep")}
          </Button>
          <Button variant="destructive" disabled={busy} onClick={() => void revoke()}>
            {t("nodes.confirmRevoke")}
          </Button>
        </div>
      </Modal>
    </div>
  );
}
