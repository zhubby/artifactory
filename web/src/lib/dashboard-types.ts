/**
 * [INPUT]: 依赖控制平面完整窗口的构建聚合响应。
 * [OUTPUT]: 提供概览窗口、指标、日期桶和近期构建活动类型。
 * [POS]: 租户与项目概览共用的数据边界，不从构建列表推算统计。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export interface DashboardBuild {
  id: string;
  number: number;
  status: string;
  release_id: string;
  release_version: string;
  project_id: string;
  project_name: string;
  project_slug: string;
  created_at: string;
  finished_at: string | null;
  activity_at: string;
}

export interface DashboardDay {
  date: string;
  total: number;
  succeeded: number;
  failed: number;
  canceled: number;
  ongoing: number;
  median_turnaround_seconds: number | null;
}

export interface DashboardData {
  window: {
    days: number;
    timezone: string;
    start: string;
    end: string;
    generated_at: string;
  };
  summary: {
    total_projects: number;
    active_projects: number;
    builds_created: number;
    succeeded: number;
    failed: number;
    canceled: number;
    ongoing: number;
    current_ongoing: number;
    success_rate: number | null;
    median_turnaround_seconds: number | null;
  };
  daily: DashboardDay[];
  recent_projects: {
    id: string;
    name: string;
    slug: string;
    status: string;
    activity_at: string;
    last_build: DashboardBuild;
  }[];
  recent_builds: DashboardBuild[];
}
