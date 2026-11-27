/**
 * [INPUT]: 依赖服务端日期桶、Recharts 与共享语言和主题变量。
 * [OUTPUT]: 提供圆角渐变构建柱、柔和周转曲线与对应空态。
 * [POS]: 两层概览的图表入口；保留真实比例、日期断点与孤立样本。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useId } from "react";
import { ChartNoAxesColumn, Timer } from "lucide-react";
import {
  Bar, BarChart, CartesianGrid, Dot, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis,
  type BarShapeProps,
} from "recharts";
import { useUI } from "@/components/providers";
import type { DashboardData } from "@/lib/dashboard-types";

const chartMargin = { top: 12, right: 12, left: 0, bottom: 0 };
const axisTick = { fill: "var(--muted-foreground)", fontSize: 11 };
const turnaroundColor = "var(--chart-turnaround)";

function RoundedResultBar({ x, y, width, height, fill, isActive }: BarShapeProps) {
  if (![x, y, width, height].every(Number.isFinite) || !width || !height) return null;
  const barWidth = Math.abs(width);
  const barHeight = Math.abs(height);
  // 圆角收敛到实际高度；不使用最小高度放大小比重。
  const radius = Math.min(8, barWidth / 2, barHeight / 2);
  return <rect
    x={width < 0 ? x + width : x}
    y={height < 0 ? y + height : y}
    width={barWidth}
    height={barHeight}
    rx={radius}
    ry={radius}
    fill={fill}
    stroke="var(--card)"
    strokeWidth={Math.min(0.8, barHeight * 0.12)}
    className="dashboard-result-bar"
    data-active={isActive || undefined}
  />;
}

export default function DashboardCharts({ data }: { data: DashboardData }) {
  const { t, locale, number } = useUI();
  const id = useId();
  const resultTitle = `${id}-results`;
  const turnaroundTitle = `${id}-turnaround`;
  const gradientId = id.replace(/:/g, "");
  const lineGradient = `${gradientId}-turnaround-gradient`;
  const dateFormat = new Intl.DateTimeFormat(locale, {
    month: "short", day: "numeric", timeZone: "UTC",
  });
  const date = (value: string) => dateFormat.format(new Date(`${value}T12:00:00Z`));
  const seconds = (value: number) => new Intl.NumberFormat(locale, {
    style: "unit", unit: value >= 3600 ? "hour" : value >= 60 ? "minute" : "second",
    unitDisplay: "short", maximumFractionDigits: 1,
  }).format(value / (value >= 3600 ? 3600 : value >= 60 ? 60 : 1));
  const series = [
    { key: "succeeded", label: t("status.succeeded"), color: "var(--chart-succeeded)" },
    { key: "failed", label: t("status.failed"), color: "var(--chart-failed)" },
    { key: "canceled", label: t("status.canceled"), color: "var(--chart-canceled)" },
    { key: "ongoing", label: t("dashboard.ongoing"), color: "var(--chart-ongoing)" },
  ] as const;
  const axis = { tickLine: false, axisLine: false, tick: axisTick, tickMargin: 8 };
  const dateAxis = { ...axis, dataKey: "date", tickFormatter: date, minTickGap: 24, height: 30, interval: "preserveStartEnd" as const };
  const tooltip = {
    contentStyle: { background: "var(--card)", border: "1px solid var(--border)", borderRadius: 16, padding: "12px 15px", fontSize: 12 },
    labelStyle: { color: "var(--foreground)", fontWeight: 600 },
    itemStyle: { color: "var(--foreground)", padding: "2px 0" },
    labelFormatter: (label: unknown) => typeof label === "string" ? date(label) : "",
    isAnimationActive: false,
  };
  const hasBuilds = data.summary.builds_created > 0;
  const hasDuration = data.daily.some(day => day.median_turnaround_seconds !== null);
  const longestDuration = Math.max(0, ...data.daily.map(day => day.median_turnaround_seconds ?? 0));
  const axisUnit = longestDuration >= 3600 ? "hour" : longestDuration >= 60 ? "minute" : "second";
  const unitSeconds = axisUnit === "hour" ? 3600 : axisUnit === "minute" ? 60 : 1;
  const durationAxisFormat = new Intl.NumberFormat(locale, {
    style: "unit", unit: axisUnit, unitDisplay: "short", maximumFractionDigits: 1,
  });

  return <div className="dashboard-charts" data-empty={!hasBuilds && !hasDuration}>
    <section className="dashboard-chart-panel" aria-labelledby={resultTitle}>
      <header><h2 id={resultTitle}>{t("dashboard.dailyResults")}</h2><p>{t("dashboard.dailyResultsHint")}</p></header>
      <div className="dashboard-chart">
        {hasBuilds ? <ResponsiveContainer width="100%" height="100%" minWidth={0}>
          <BarChart data={data.daily} accessibilityLayer margin={chartMargin} barCategoryGap="38%">
            <defs>{series.map(item => <linearGradient key={item.key} id={`${gradientId}-${item.key}`} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={`color-mix(in srgb, ${item.color} 82%, white)`} />
              <stop offset="100%" stopColor={item.color} />
            </linearGradient>)}</defs>
            <CartesianGrid stroke="var(--chart-grid)" strokeDasharray="3 5" vertical={false} />
            <XAxis {...dateAxis} />
            <YAxis {...axis} width="auto" tickCount={5} allowDecimals={false} tickFormatter={number} />
            <Tooltip {...tooltip} cursor={{ fill: "var(--row-hover)", radius: 8 }} formatter={value => typeof value === "number" ? number(value) : "—"} />
            {series.map(item => <Bar key={item.key} dataKey={item.key} name={item.label} stackId="results"
              fill={`url(#${gradientId}-${item.key})`} shape={RoundedResultBar} activeBar={RoundedResultBar}
              isAnimationActive={false} maxBarSize={30} />)}
          </BarChart>
        </ResponsiveContainer> : <div className="dashboard-chart-empty">
          <ChartNoAxesColumn size={20} aria-hidden="true" />
          <p>{t("dashboard.noBuilds")}</p>
        </div>}
      </div>
      {hasBuilds && <footer className="dashboard-chart-footer">
        <ul className="dashboard-chart-legend" aria-label={t("dashboard.resultLegend")}>{series.map(item => <li key={item.key}>
          <span style={{ background: item.color }} aria-hidden="true" />{item.label}
        </li>)}</ul>
        <p>{t("dashboard.resultsNote")}</p>
      </footer>}
      {hasBuilds && <ul className="sr-only" aria-label={t("dashboard.dailyResultData")}>{data.daily.map(day => <li key={day.date}>
        {date(day.date)}: {series.map(item => `${item.label} ${number(day[item.key])}`).join("; ")}
      </li>)}</ul>}
    </section>
    <section className="dashboard-chart-panel" aria-labelledby={turnaroundTitle}>
      <header><h2 id={turnaroundTitle}>{t("dashboard.turnaroundTrend")}</h2><p>{t("dashboard.turnaroundTrendHint")}</p></header>
      <div className="dashboard-chart">
        {hasDuration ? <ResponsiveContainer width="100%" height="100%" minWidth={0}>
          <LineChart data={data.daily} accessibilityLayer margin={chartMargin}>
            <defs><linearGradient id={lineGradient} gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="640" y2="0">
              <stop offset="0%" stopColor={`color-mix(in srgb, ${turnaroundColor} 84%, white)`} />
              <stop offset="55%" stopColor={turnaroundColor} />
              <stop offset="100%" stopColor={`color-mix(in srgb, ${turnaroundColor} 88%, var(--primary))`} />
            </linearGradient></defs>
            <CartesianGrid stroke="var(--chart-grid)" strokeDasharray="3 5" vertical={false} />
            <XAxis {...dateAxis} padding={{ left: 4, right: 4 }} />
            <YAxis {...axis} width="auto" tickCount={5} domain={[0, "auto"]}
              tickFormatter={(value: number) => durationAxisFormat.format(value / unitSeconds)} />
            <Tooltip {...tooltip} cursor={{ stroke: "var(--ring)", strokeDasharray: "3 3" }}
              formatter={value => typeof value === "number" ? seconds(value) : "—"} />
            <Line dataKey="median_turnaround_seconds" name={t("dashboard.median")} type="monotoneX"
              stroke={`url(#${lineGradient})`} strokeWidth={3} strokeLinecap="round" strokeLinejoin="round"
              className="dashboard-turnaround-line"
              dot={({ cx, cy, index }) => {
                if (data.daily[index]?.median_turnaround_seconds == null) return null;
                const previous = data.daily[index - 1]?.median_turnaround_seconds;
                const next = data.daily[index + 1]?.median_turnaround_seconds;
                if (previous != null || next != null) return null;
                return <Dot cx={cx} cy={cy} r={4} fill={turnaroundColor} stroke="var(--card)" strokeWidth={2} className="dashboard-chart-sample" />;
              }}
              activeDot={{ r: 5, fill: turnaroundColor, stroke: "var(--card)", strokeWidth: 2 }} connectNulls={false} isAnimationActive={false} />
          </LineChart>
        </ResponsiveContainer> : <div className="dashboard-chart-empty">
          <Timer size={20} aria-hidden="true" />
          <p>{t("dashboard.noSuccessfulBuilds")}</p>
        </div>}
      </div>
      {hasDuration && <footer className="dashboard-chart-footer"><p>{t("dashboard.turnaroundNote")}</p></footer>}
      {hasDuration && <ul className="sr-only" aria-label={t("dashboard.turnaroundData")}>{data.daily.map(day => <li key={day.date}>
        {date(day.date)}: {day.median_turnaround_seconds === null ? t("dashboard.noSample") : seconds(day.median_turnaround_seconds)}
      </li>)}</ul>}
    </section>
  </div>;
}
