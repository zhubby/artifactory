/**
 * [INPUT]: 依赖租户 API、请求取消和本地化错误。
 * [OUTPUT]: 提供项目引用查询及显式刷新。
 * [POS]: 构建引导的数据入口；隔离项目切换前的迟到响应。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useEffect, useState } from "react";
import { api } from "@/lib/api";
import { asError } from "@/lib/errors";

type GitRef = { name: string; ref: string; oid: string };
type RepositoryRefs = {
  default_ref: string | null;
  branches: GitRef[];
  tags: GitRef[];
};

export function useRepositoryRefs(projectID: string) {
  const [revision, setRevision] = useState(0);
  const [snapshot, setSnapshot] = useState<{
    projectID: string;
    data?: RepositoryRefs;
    error?: Error;
    loading: boolean;
  }>({ projectID: "", loading: false });
  useEffect(() => {
    if (!projectID) return;
    let active = true;
    const controller = new AbortController();
    setSnapshot({ projectID, loading: true });
    void api<RepositoryRefs>(
      `/projects/${encodeURIComponent(projectID)}/refs`,
      { signal: controller.signal },
    )
      .then((data) => {
        if (active) setSnapshot({ projectID, data, loading: false });
      })
      .catch((error) => {
        if (active)
          setSnapshot({ projectID, error: asError(error), loading: false });
      });
    return () => {
      active = false;
      controller.abort();
    };
  }, [projectID, revision]);
  const current = snapshot.projectID === projectID ? snapshot : undefined;
  return {
    data: current?.data,
    error: current?.error,
    loading: current?.loading ?? Boolean(projectID),
    reload: () => setRevision((previous) => previous + 1),
  };
}
