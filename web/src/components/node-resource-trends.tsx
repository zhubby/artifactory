/**
 * [INPUT]: 依赖节点分钟遥测、共享请求、下拉与图表主题变量。
 * [OUTPUT]: 提供 CPU 和已用内存趋势、范围切换与刷新。
 * [POS]: 资源分区按需加载的历史视图，使用单调曲线展示采样和断点。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useId, useState } from "react";
import { RefreshCw } from "lucide-react";
import { Area, AreaChart, CartesianGrid, Dot, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { Loading } from "@/components/common";
import { NodeDataError } from "@/components/node-detail-parts";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { useRemote } from "@/lib/api";
import { bytes } from "@/lib/utils";

const minute = 60_000;
const ranges = [15, 60, 360] as const;
const axis = {
  tick: { fill: "var(--muted-foreground)", fontSize: 11 },
  tickLine: false,
  axisLine: false,
  tickMargin: 8,
};

interface HostHistory {
  samples: { bucket: string; cpu_avg: number | null; memory_avg: number | null }[];
  truncated: boolean;
}

type TrendPoint = { time: number; cpu: number | null; memory: number | null };

function currentMinute() {
  return Math.floor(Date.now() / minute) * minute;
}

function TrendChart({ title, field, color, data, from, to }: {
  title: string;
  field: "cpu" | "memory";
  color: string;
  data: TrendPoint[];
  from: number;
  to: number;
}) {
  const { t, locale } = useUI();
  const id = useId();
  const time = new Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit" });
  const dateTime = new Intl.DateTimeFormat(locale, { dateStyle: "short", timeStyle: "short" });
  const percent = new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: 1 });
  const formatValue = field === "cpu" ? (value: number) => percent.format(value / 100)
    : (value: number) => bytes(Math.round(value));
  const hasData = data.some(point => point[field] !== null);

  return (
    <section className="node-trend-card" aria-labelledby={id}>
      <h4 id={id}>{title}</h4>
      {hasData ? (
        <div className="node-trend-plot">
          <ResponsiveContainer width="100%" height="100%" minWidth={0}>
            <AreaChart data={data} accessibilityLayer aria-label={title} margin={{ top: 8, right: 12, bottom: 0, left: 0 }}>
              <CartesianGrid stroke="var(--border)" vertical={false} />
              <XAxis {...axis} dataKey="time" type="number" scale="time" domain={[from, to]}
                ticks={[from, from + Math.floor((to - from) / (2 * minute)) * minute, to]}
                height={27} interval="preserveStartEnd" tickFormatter={value => time.format(value)} />
              <YAxis {...axis} width="auto" tickCount={4} domain={field === "cpu" ? [0, 100] : [0, "auto"]}
                ticks={field === "cpu" ? [0, 25, 50, 75, 100] : undefined}
                allowDecimals={false} tickFormatter={formatValue} />
              <Tooltip
                isAnimationActive={false}
                contentStyle={{ background: "var(--card)", border: "1px solid var(--border)", borderRadius: 6, fontSize: 12 }}
                labelStyle={{ color: "var(--foreground)", fontWeight: 600 }}
                itemStyle={{ color: "var(--foreground)" }}
                cursor={{ stroke: "var(--ring)", strokeDasharray: "3 3" }}
                labelFormatter={value => typeof value === "number" ? dateTime.format(value) : ""}
                formatter={value => typeof value === "number" ? formatValue(value) : t("nodes.notReported")}
              />
              <Area name={title} dataKey={field} type="monotoneX" stroke={color} fill={color}
                strokeWidth={1.75} strokeLinecap="round" fillOpacity={0.09} connectNulls={false} isAnimationActive={false}
                dot={({ cx, cy, index }) => {
                  if (data[index - 1]?.[field] != null || data[index + 1]?.[field] != null) return null;
                  return <Dot cx={cx} cy={cy} r={3} fill={color} />;
                }}
                activeDot={{ r: 4, stroke: "var(--card)", strokeWidth: 2 }} />
            </AreaChart>
          </ResponsiveContainer>
        </div>
      ) : <p className="node-trend-empty">{t("nodes.noTrendSamples")}</p>}
    </section>
  );
}

export default function NodeResourceTrends({ nodeId }: { nodeId: string }) {
  const { t } = useUI();
  const id = useId();
  const [{ minutes, to }, setWindow] = useState(() => ({ minutes: 60, to: currentMinute() }));
  const from = to - minutes * minute;
  const query = new URLSearchParams({
    from: new Date(from).toISOString(),
    to: new Date(to).toISOString(),
    resolution: "minute",
  });
  const history = useRemote<HostHistory>(`/nodes/${nodeId}/telemetry/history?${query}`, false);
  const byMinute = new Map(history.data?.samples.map(sample => [Date.parse(sample.bucket), sample]));
  const data = Array.from({ length: minutes }, (_, index) => {
    const time = from + index * minute;
    const sample = byMinute.get(time);
    return { time, cpu: sample?.cpu_avg ?? null, memory: sample?.memory_avg ?? null };
  });

  function refresh() {
    const end = currentMinute();
    if (to === end) void history.reload();
    else setWindow({ minutes, to: end });
  }

  return (
    <section className="node-detail-section node-resource-trends" aria-labelledby={id}>
      <header className="node-trend-header">
        <div>
          <h3 id={id}>{t("nodes.resourceTrends")}</h3>
          <p>{t("nodes.trendInterval")}</p>
        </div>
        <div className="node-trend-controls">
          <Select aria-label={t("nodes.trendRange")} value={minutes}
            onChange={event => setWindow({ minutes: Number(event.target.value), to: currentMinute() })}>
            {ranges.map(range => <option key={range} value={range}>
              {t(range < 60 ? "nodes.lastMinutes" : "nodes.lastHours", { count: range < 60 ? range : range / 60 })}
            </option>)}
          </Select>
          <Button variant="secondary" size="icon" aria-label={t("nodes.refreshTrends")}
            title={t("nodes.refreshTrends")} onClick={refresh} disabled={!history.data && !history.error}>
            <RefreshCw size={15} aria-hidden="true" />
          </Button>
        </div>
      </header>
      <NodeDataError error={history.error} retry={history.reload} />
      {!history.data && !history.error && <Loading variant="cards" />}
      {history.data && <div className="node-trend-grid">
        <TrendChart title={`CPU ${t("nodes.cpuUsage")}`} field="cpu" color="var(--chart-turnaround)" data={data} from={from} to={to} />
        <TrendChart title={t("nodes.memoryUsed")} field="memory" color="var(--chart-succeeded)" data={data} from={from} to={to} />
      </div>}
      {history.data?.truncated && <p className="node-section-note">{t("nodes.trendsTruncated")}</p>}
    </section>
  );
}
