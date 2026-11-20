/**
 * [INPUT]: 依赖节点 API、通知、Select、工具栏和节点侧面板。
 * [OUTPUT]: 提供 NodesWorkspace 设备工作区。
 * [POS]: 衔接节点筛选、列表与详情交互。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useEffect, useState } from "react";
import { Plus } from "lucide-react";
import { useUI } from "@/components/providers";
import { useRemote, mutation } from "@/lib/api";
import { notify } from "@/lib/notify";
import type { Node, Pool, User } from "@/lib/types";
import { NodeCard } from "@/components/node-card";
import { NodeDetailDialog } from "@/components/node-detail-dialog";
import { SecretDialog, SubmitForm } from "@/components/forms";
import { Modal } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { ListToolbar } from "@/components/ui/list-toolbar";
import {
  PageTitle,
  ErrorBox,
  Loading,
  Empty,
  Field,
} from "@/components/common";

export function NodesWorkspace({
  user,
  readOnly,
}: {
  user: User;
  readOnly: boolean;
}) {
  const { t } = useUI();
  const [query, setQuery] = useState("");
  const [offset, setOffset] = useState(0);
  const [selection, setSelection] = useState<string>();
  const [enroll, setEnroll] = useState(false);
  const [secret, setSecret] = useState<string>();
  const nodes = useRemote<Node[]>(
    `/nodes?limit=100&offset=${offset}&q=${encodeURIComponent(query)}`,
  );
  const pools = useRemote<Pool[]>("/pools");
  const selected = nodes.data?.find((node) => node.id === selection);
  useEffect(() => {
    setSelection(undefined);
  }, [query, offset]);
  useEffect(() => {
    if (selection && nodes.data && !selected) {
      setSelection(undefined);
      notify.info(t("nodes.noLongerListed"));
    }
  }, [selection, nodes.data, selected, t]);
  return (
    <>
      <PageTitle
        eyebrow={t("workspace.tenant")}
        title={t("workspace.sharedDevices")}
        description={t("nodes.description")}
      >
        {user.role === "admin" && !readOnly && (
          <Button onClick={() => setEnroll(true)}>
            <Plus />
            {t("nodes.register")}
          </Button>
        )}
      </PageTitle>
      <ListToolbar query={query} count={nodes.data?.length}
          onQueryChange={(value) => {
            setQuery(value);
            setOffset(0);
          }}
      />
      <ErrorBox message={nodes.error || pools.error} />
      {!nodes.data ? (
        !nodes.error && <Loading />
      ) : !nodes.data.length ? (
        <section className="panel">
          <Empty title={t("nodes.empty")} detail={t("nodes.emptyDetail")} />
        </section>
      ) : (
        <div className="node-grid">
          {nodes.data.map((node) => (
            <NodeCard
              key={node.id}
              node={node}
              onDetails={() => {
                setSelection(node.id);
              }}
            />
          ))}
        </div>
      )}
      <nav
        className="flex items-center justify-end gap-3 mt-5"
        aria-label={t("nodes.pages")}
      >
        <Button
          variant="secondary"
          disabled={!nodes.data || offset === 0}
          onClick={() => setOffset(Math.max(0, offset - 100))}
        >
          {t("nodes.previous")}
        </Button>
        <span>{t("nodes.page", { page: offset / 100 + 1 })}</span>
        <Button
          variant="secondary"
          disabled={!nodes.data || nodes.data.length < 100}
          onClick={() => setOffset(offset + 100)}
        >
          {t("nodes.next")}
        </Button>
      </nav>
      {selected && (
        <NodeDetailDialog
          key={selected.id}
          node={selected}
          user={readOnly ? { ...user, role: "viewer" } : user}
          onClose={() => setSelection(undefined)}
          reload={nodes.reload}
          pools={pools.data || []}
        />
      )}
      <Modal
        open={enroll}
        onOpenChange={setEnroll}
        title={t("nodes.register")}
        description={t("nodes.dialogDetail")}
      >
        <SubmitForm
          label={t("security.createToken")}
          onSubmit={async (form) => {
            const result = await mutation<{ token: string }>(
              "/enrollment-tokens",
              { pool_id: form.get("pool_id") },
            );
            setEnroll(false);
            setSecret(result.token);
          }}
        >
          <Field label={t("nodes.joinPool")}>
            <Select name="pool_id" className="input">
              {pools.data?.map((pool) => (
                <option key={pool.id} value={pool.id}>
                  {pool.name}
                </option>
              ))}
            </Select>
          </Field>
        </SubmitForm>
      </Modal>
      <SecretDialog
        value={secret}
        title={t("nodes.saveToken")}
        close={() => setSecret(undefined)}
      />
    </>
  );
}
