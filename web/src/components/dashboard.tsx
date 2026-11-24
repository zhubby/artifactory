/**
 * [INPUT]: 依赖完整窗口统计 API、项目身份、共享下拉与租户路由。
 * [OUTPUT]: 提供两层概览的指标定义列表、趋势和近期构建活动。
 * [POS]: 只读取服务端聚合；图表延迟加载，页面标题和创建操作由工作区提供。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import dynamic from "next/dynamic";
import { useEffect, useState } from "react";
import { ArrowRight, ArrowUpRight, Clock3, FolderGit2 } from "lucide-react";
import { Empty, ErrorBox, Loading, Status } from "@/components/common";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { useRemote } from "@/lib/api";
import type { DashboardData } from "@/lib/dashboard-types";
import { buildPath, projectPath } from "@/lib/routes";
import { Link } from "@/lib/tenant";
import type { Project } from "@/lib/types";

const DashboardCharts = dynamic(() => import("@/components/dashboard-charts"), {
  ssr: false,
  loading: () => <div className="dashboard-charts dashboard-chart-loading"><Loading variant="cards" /></div>,
});

function duration(seconds: number | null, locale: string) {
  if (seconds === null) return "—";
  const unit = seconds >= 3600 ? "hour" : seconds >= 60 ? "minute" : "second";
  const value = seconds / (unit === "hour" ? 3600 : unit === "minute" ? 60 : 1);
  return new Intl.NumberFormat(locale, {
    style: "unit", unit, unitDisplay: "short", maximumFractionDigits: value < 10 ? 1 : 0,
  }).format(value);
}

function DashboardMetrics({ data, project }: { data: DashboardData; project?: Project }) {
  const { t, locale, number } = useUI();
  const summary = data.summary;
  const metrics = [
    ...(!project ? [{
      label: t("dashboard.activeProjects"),
      value: number(summary.active_projects),
      suffix: `/ ${number(summary.total_projects)}`,
      detail: t("dashboard.activeProjectsHint"),
    }] : []),
    { label: t("dashboard.buildsCreated"), value: number(summary.builds_created), detail: t("dashboard.buildsCreatedHint") },
    {
      label: t("dashboard.successRate"),
      value: summary.success_rate === null ? "—" : new Intl.NumberFormat(locale, {
        style: "percent", maximumFractionDigits: 1,
      }).format(summary.success_rate),
      detail: t("dashboard.successRateHint"),
    },
    ...(project ? [{
      label: t("dashboard.turnaround"),
      value: duration(summary.median_turnaround_seconds, locale),
      detail: t("dashboard.turnaroundHint"),
    }] : []),
    { label: t("dashboard.currentOngoing"), value: number(summary.current_ongoing), detail: t("dashboard.currentOngoingHint") },
  ];
  return <div className="dashboard-metrics">
    {metrics.map(metric => <div className="dashboard-metric" key={metric.label}>
      <dl>
        <dt>{metric.label}</dt>
        <dd>{metric.value}{"suffix" in metric && <span>{metric.suffix}</span>}</dd>
      </dl>
      <p>{metric.detail}</p>
    </div>)}
  </div>;
}

function DashboardActivity({ data, project }: { data: DashboardData; project?: Project }) {
  const { t, date } = useUI();
  const empty = project ? !data.recent_builds.length : !data.recent_projects.length;
  return <section className="dashboard-activity" aria-labelledby="dashboard-activity-heading">
    <header className="dashboard-section-heading">
      <div>
        <h2 id="dashboard-activity-heading">{t(project ? "dashboard.recentBuilds" : "dashboard.recentProjects")}</h2>
        <p>{t("dashboard.activityHint")}</p>
      </div>
      <Link href={project ? `${projectPath(project.slug)}/builds` : "/projects"}>
        {t(project ? "dashboard.allBuilds" : "dashboard.allProjects")}<ArrowUpRight size={14} aria-hidden="true" />
      </Link>
    </header>
    {empty ? <Empty title={t("dashboard.noActivity")} detail={t(project ? "dashboard.noBuildActivityHint" : "dashboard.noProjectActivityHint")}
      action={!project && <Button asChild variant="secondary"><Link href="/projects">{t("dashboard.allProjects")}</Link></Button>} /> : (
      <ul className="dashboard-activity-list">
        {project ? data.recent_builds.map(build => <li key={build.id}>
          <span className="dashboard-activity-icon"><Clock3 size={16} aria-hidden="true" /></span>
          <div className="dashboard-activity-identity">
            <Link href={buildPath(build.project_slug, build.number, build.release_id)}>
              {t("buildDetail.title", { number: build.number })}<ArrowUpRight size={13} aria-hidden="true" />
            </Link>
            <p className="font-mono">{build.release_version}</p>
          </div>
          <Status small value={build.status} />
          <time dateTime={build.activity_at}>{date(build.activity_at)}</time>
        </li>) : data.recent_projects.map(item => <li key={item.id}>
          <span className="dashboard-activity-icon"><FolderGit2 size={17} aria-hidden="true" /></span>
          <div className="dashboard-activity-identity">
            <Link href={projectPath(item.slug)}>{item.name}<ArrowUpRight size={13} aria-hidden="true" /></Link>
            <Link className="dashboard-activity-build" href={buildPath(item.slug, item.last_build.number, item.last_build.release_id)}>
              #{item.last_build.number}<span className="font-mono">{item.last_build.release_version}</span>
            </Link>
          </div>
          <Status small value={item.last_build.status} />
          <time dateTime={item.activity_at}>{date(item.activity_at)}</time>
        </li>)}
      </ul>
    )}
    {project && <footer className="dashboard-activity-footer">
      <Link href={`${projectPath(project.slug)}/artifacts`}>{t("navigation.artifacts")}<ArrowRight size={14} aria-hidden="true" /></Link>
    </footer>}
  </section>;
}

export function Dashboard({ project }: { project?: Project }) {
  const { t } = useUI();
  const [days, setDays] = useState("30");
  const [timezone, setTimezone] = useState<string>();
  useEffect(() => {
    setTimezone(Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC");
  }, []);
  const query = new URLSearchParams({ days, timezone: timezone || "UTC" });
  if (project) query.set("project_id", project.id);
  const remote = useRemote<DashboardData>(timezone ? `/dashboard?${query}` : null, 10000);
  const data = remote.data;
  return <section className="dashboard" aria-label={t(project ? "dashboard.projectOverview" : "dashboard.tenantOverview")}>
    <div className="dashboard-toolbar">
      <p>{data ? t("dashboard.windowHint", { timezone: data.window.timezone }) : t("dashboard.periodHint")}</p>
      <Select aria-label={t("dashboard.period")} value={days} onChange={event => setDays(event.target.value)}>
        {[7, 30, 90].map(value => <option key={value} value={String(value)}>{t("dashboard.lastDays", { days: value })}</option>)}
      </Select>
    </div>
    <ErrorBox message={remote.error} />
    {remote.error && <div className="dashboard-retry"><Button size="sm" variant="secondary" onClick={() => void remote.reload()}>{t("common.retry")}</Button></div>}
    {!data ? !remote.error && <Loading variant="detail" /> : <>
      <DashboardMetrics data={data} project={project} />
      <DashboardCharts data={data} />
      <DashboardActivity data={data} project={project} />
    </>}
  </section>;
}
