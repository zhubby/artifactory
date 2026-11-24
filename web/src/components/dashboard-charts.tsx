/**
 * [INPUT]: 依赖服务端日期桶、Recharts 与共享语言和主题变量。
 * [OUTPUT]: 提供具名图表分区中的构建结果和成功周转趋势。
 * [POS]: 概览按需加载的图表边界；不推算统计，空值保留为折线断点。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useId } from "react";
import {
  Bar, BarChart, CartesianGrid, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis,
} from "recharts";
import { useUI } from "@/components/providers";
import type { DashboardData } from "@/lib/dashboard-types";

export default function DashboardCharts({ data }: { data: DashboardData }) {
  const { t, locale, number } = useUI();
  const id = useId();
  const resultTitle = `${id}-results`;
  const turnaroundTitle = `${id}-turnaround`;
  const date = (value: string) => new Intl.DateTimeFormat(locale, {
    month: "short", day: "numeric", timeZone: "UTC",
  }).format(new Date(`${value}T12:00:00Z`));
  const seconds = (value: number) => new Intl.NumberFormat(locale, {
    style: "unit", unit: value >= 3600 ? "hour" : value >= 60 ? "minute" : "second",
    unitDisplay: "short", maximumFractionDigits: 1,
  }).format(value / (value >= 3600 ? 3600 : value >= 60 ? 60 : 1));
  const series = [
    { key: "succeeded", label: t("status.succeeded"), color: "var(--success)" },
    { key: "failed", label: t("status.failed"), color: "var(--destructive)" },
    { key: "canceled", label: t("status.canceled"), color: "var(--muted-foreground)" },
    { key: "ongoing", label: t("dashboard.ongoing"), color: "var(--warning)" },
  ] as const;
  const tooltip = {
    contentStyle: { background: "var(--card)", border: "1px solid var(--border)", borderRadius: 5, fontSize: 11 },
    labelStyle: { color: "var(--foreground)", marginBottom: 6 },
  };
  const hasBuilds = data.summary.builds_created > 0;
  const hasDuration = data.daily.some(day => day.median_turnaround_seconds !== null);
  return <div className="dashboard-charts">
    <section className="dashboard-chart-panel" aria-labelledby={resultTitle}>
      <header><h2 id={resultTitle}>{t("dashboard.dailyResults")}</h2><p>{t("dashboard.dailyResultsHint")}</p></header>
      <div className="dashboard-chart">
        <ResponsiveContainer width="100%" height="100%" minWidth={0}>
          <BarChart data={data.daily} accessibilityLayer margin={{ top: 10, right: 8, left: -24, bottom: 0 }} barCategoryGap="25%">
            <CartesianGrid stroke="var(--border)" vertical={false} strokeDasharray="3 3" />
            <XAxis dataKey="date" tickFormatter={date} tickLine={false} axisLine={false} minTickGap={30} tick={{ fill: "var(--muted-foreground)", fontSize: 10 }} />
            <YAxis allowDecimals={false} tickLine={false} axisLine={false} tick={{ fill: "var(--muted-foreground)", fontSize: 10 }} tickFormatter={number} />
            <Tooltip {...tooltip} cursor={{ fill: "var(--quiet)" }} labelFormatter={label => date(String(label))} />
            {series.map(item => <Bar key={item.key} dataKey={item.key} name={item.label} stackId="results" fill={item.color} isAnimationActive={false} maxBarSize={28} />)}
          </BarChart>
        </ResponsiveContainer>
        {!hasBuilds && <p className="dashboard-chart-empty">{t("dashboard.noBuilds")}</p>}
      </div>
      <ul className="dashboard-chart-legend" aria-label={t("dashboard.resultLegend")}>{series.map(item => <li key={item.key}>
        <span style={{ background: item.color }} aria-hidden="true" />{item.label}
      </li>)}</ul>
      <p className="dashboard-chart-note">{t("dashboard.resultsNote")}</p>
      <ul className="sr-only" aria-label={t("dashboard.dailyResultData")}>{data.daily.map(day => <li key={day.date}>
        {date(day.date)}: {series.map(item => `${item.label} ${number(day[item.key])}`).join("; ")}
      </li>)}</ul>
    </section>
    <section className="dashboard-chart-panel" aria-labelledby={turnaroundTitle}>
      <header><h2 id={turnaroundTitle}>{t("dashboard.turnaroundTrend")}</h2><p>{t("dashboard.turnaroundTrendHint")}</p></header>
      <div className="dashboard-chart">
        <ResponsiveContainer width="100%" height="100%" minWidth={0}>
          <LineChart data={data.daily} accessibilityLayer margin={{ top: 10, right: 10, left: 0, bottom: 0 }}>
            <CartesianGrid stroke="var(--border)" vertical={false} strokeDasharray="3 3" />
            <XAxis dataKey="date" tickFormatter={date} tickLine={false} axisLine={false} minTickGap={30} tick={{ fill: "var(--muted-foreground)", fontSize: 10 }} />
            <YAxis width={72} tickLine={false} axisLine={false} tickFormatter={seconds} tick={{ fill: "var(--muted-foreground)", fontSize: 10 }} domain={[0, "auto"]} />
            <Tooltip {...tooltip} cursor={{ stroke: "var(--border)" }} labelFormatter={label => date(String(label))}
              formatter={value => typeof value === "number" ? seconds(value) : "—"} />
            <Line dataKey="median_turnaround_seconds" name={t("dashboard.turnaround")} type="linear" stroke="var(--foreground)" strokeWidth={2}
              dot={{ r: 2, fill: "var(--card)", strokeWidth: 1.5 }} activeDot={{ r: 4 }} connectNulls={false} isAnimationActive={false} />
          </LineChart>
        </ResponsiveContainer>
        {!hasDuration && <p className="dashboard-chart-empty">{t("dashboard.noSuccessfulBuilds")}</p>}
      </div>
      <p className="dashboard-turnaround-key"><span aria-hidden="true" />{t("dashboard.median")}</p>
      <p className="dashboard-chart-note">{t("dashboard.turnaroundNote")}</p>
      <ul className="sr-only" aria-label={t("dashboard.turnaroundData")}>{data.daily.map(day => <li key={day.date}>
        {date(day.date)}: {day.median_turnaround_seconds === null ? t("dashboard.noSample") : seconds(day.median_turnaround_seconds)}
      </li>)}</ul>
    </section>
  </div>;
}
