/**
 * [INPUT]: 依赖节点池筛选 URL、节点 API、通知、安装弹窗与侧面板。
 * [OUTPUT]: 提供 NodesWorkspace 设备工作区。
 * [POS]: 衔接节点筛选、列表与详情交互。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useEffect, useState } from "react";
import { useSearchParams } from "next/navigation";
import { Plus } from "lucide-react";
import { useUI } from "@/components/providers";
import { useRemote } from "@/lib/api";
import { notify } from "@/lib/notify";
import type { Node, Pool, User } from "@/lib/types";
import { NodeCard } from "@/components/node-card";
import { NodeDetailDialog } from "@/components/node-detail-dialog";
import { NodeEnrollmentDialog } from "@/components/node-enrollment-dialog";
import { Button } from "@/components/ui/button";
import { ListToolbar } from "@/components/ui/list-toolbar";
import { Select } from "@/components/ui/select";
import { useRouter } from "@/lib/tenant";
import {
  PageTitle,
  ErrorBox,
  Loading,
  Empty,
} from "@/components/common";

export function NodesWorkspace({
  user,
  readOnly,
}: {
  user: User;
  readOnly: boolean;
}) {
  const { t } = useUI();
  const parameters = useSearchParams();
  const router = useRouter();
  const pool = parameters.get("pool") || "";
  const [query, setQuery] = useState("");
  const [offset, setOffset] = useState(0);
  const [selection, setSelection] = useState<string>();
  const [enroll, setEnroll] = useState(false);
  const nodes = useRemote<Node[]>(
    `/nodes?limit=100&offset=${offset}&q=${encodeURIComponent(query)}${pool ? `&pool_id=${encodeURIComponent(pool)}` : ""}`,
  );
  const pools = useRemote<Pool[]>("/pools");
  const selected = nodes.data?.find((node) => node.id === selection);
  useEffect(() => {
    setSelection(undefined);
  }, [query, offset, pool]);
  useEffect(() => { setOffset(0); }, [pool]);
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
      <ListToolbar query={query} count={nodes.data?.length} placeholder={t("nodes.searchHint")}
          onQueryChange={(value) => {
            setQuery(value);
            setOffset(0);
          }}
      >
        <Select className="input" aria-label={t("nodePools.filter")} value={pool}
          onChange={event => {
            setOffset(0);
            router.replace(event.target.value ? `/nodes?pool=${encodeURIComponent(event.target.value)}` : "/nodes");
          }}>
          <option value="">{t("nodePools.all")}</option>
          {pool && !pools.data?.some(item => item.id === pool) && <option value={pool}>{pool}</option>}
          {pools.data?.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
        </Select>
      </ListToolbar>
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
      {enroll && <NodeEnrollmentDialog close={() => setEnroll(false)} />}
    </>
  );
}
