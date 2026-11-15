"use client";
import { useEffect, useState } from "react";
import { Plus, Search } from "lucide-react";
import { useUI } from "@/components/providers";
import { useRemote, mutation } from "@/lib/api";
import type { Node, Pool, User } from "@/lib/types";
import { NodeCard } from "@/components/node-card";
import { NodeDetailDialog } from "@/components/node-detail-dialog";
import { SecretDialog, SubmitForm } from "@/components/forms";
import { Modal } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
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
  const [removed, setRemoved] = useState(false);
  const nodes = useRemote<Node[]>(
    `/nodes?limit=100&offset=${offset}&q=${encodeURIComponent(query)}`,
  );
  const pools = useRemote<Pool[]>("/pools");
  const selected = nodes.data?.find((node) => node.id === selection);
  useEffect(() => {
    setSelection(undefined);
    setRemoved(false);
  }, [query, offset]);
  useEffect(() => {
    if (selection && nodes.data && !selected) {
      setSelection(undefined);
      setRemoved(true);
    }
  }, [selection, nodes.data, selected]);
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
      <label className="workspace-filter">
        <Search size={16} />
        <input
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setOffset(0);
          }}
          aria-label={t("search.label")}
          placeholder={t("search.placeholder")}
        />
      </label>
      <ErrorBox message={nodes.error} />
      {removed && (
        <p className="notice" role="status">
          {t("nodes.noLongerListed")}
        </p>
      )}
      {!nodes.data ? (
        <Loading />
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
                setRemoved(false);
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
            <select name="pool_id" className="input">
              {pools.data?.map((pool) => (
                <option key={pool.id} value={pool.id}>
                  {pool.name}
                </option>
              ))}
            </select>
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
