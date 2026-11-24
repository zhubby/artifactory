/**
 * [INPUT]: 依赖 Playwright 路由拦截与控制台业务类型。
 * [OUTPUT]: 提供构建目标、授权设备平台、概览聚合和租户 HTTP 夹具。
 * [POS]: UI 测试的默认服务响应；专项测试覆盖对应响应。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import type { Page } from "@playwright/test";
import type { DashboardBuild, DashboardData } from "../../src/lib/dashboard-types";
import type {
  Artifact,
  Build,
  Node,
  NodeTelemetry,
  Project,
  Release,
  User,
} from "../../src/lib/types";

const created = "2026-11-10T08:30:00Z";
export const tenantId = "11111111-1111-4111-8111-111111111111";
export const tenantSlug = "build-team";
export const tenantURL = (path: string, tenant = tenantSlug) =>
  /^\/(?:login|register|tenants|join|docs|notifications)(?:\/|\?|$)/.test(path)
    ? path
    : `/tenants/${tenant}${path === "/" ? "" : path}`;
export const artifact: Artifact = {
  id: "artifact-main",
  path: "dist/DesktopClient.zip",
  size: 35840,
  sha256: "abc12345".repeat(8),
  target: "macos",
  created_at: created,
  project_name: "Desktop Client",
  build_id: "build-main",
};
export const project: Project = {
  status: "active",
  archived_at: null,
  id: "project-main",
  slug: "desktop-client",
  name: "Desktop Client",
  repository_url: "https://git.example.com/team/desktop-client.git",
  credential_id: "credential-main",
  pool_ids: ["pool-main"],
  tenant_id: tenantId,
  effective_role: "admin",
  node_access: { mode: "pools", ids: ["pool-main"] },
  signing_profiles: ["apple-internal"],
  build_count: 1234,
  last_build: {
    id: "build-main",
    number: 128,
    status: "running",
    created_at: created,
  },
};
export const repositoryRefs = {
  default_ref: "refs/heads/main",
  branches: [
    { name: "main", ref: "refs/heads/main", oid: "a".repeat(40) },
    { name: "release/v1", ref: "refs/heads/release/v1", oid: "b".repeat(40) },
  ],
  tags: [{ name: "v1.2.0", ref: "refs/tags/v1.2.0", oid: "c".repeat(40) }],
};
export const build: Build = {
  goal: "构建桌面客户端并交付已签名安装包",
  target_requirements: [{kind:"native",key:"macos",os:"darwin",arch:"arm64",application_platform:"macos",artifact_key:"app",format_suffix:".zip"}],
  selected_targets: ["macos"],
  id: "build-main",
  number: 128,
  project_slug: project.slug,
  project_id: project.id,
  project_name: project.name,
  requested_ref: "main",
  commit_sha: "9d9c687dc4f55b23539aa5417b703078047efa74",
  status: "running",
  publish_status: "not_requested",
  publish_error: null,
  error: null,
  release_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  release_version: "1.2.0-internal",
  created_at: created,
  finished_at: null,
  jobs: [
    {
      id: "job-macos",
      target: "macos",
      status: "running",
      target_config: {
        os: "darwin",
        arch: "arm64",
        application_platform: "macos",
        steps: [
          { name: "compile", phase: "build" },
          { name: "verify", phase: "verify" },
        ],
      },
      attempts: [
        {
          id: "attempt-main",
          node_id: "node-main",
          node_name: "Mac Studio",
          status: "running",
          lease_expires_at: "2026-11-10T09:00:00Z",
          error: null,
          started_at: created,
          finished_at: null,
          steps: [
            {
              step_index: 0,
              name: "compile",
              phase: "build",
              status: "succeeded",
              exit_code: 0,
              started_at: created,
              finished_at: "2026-11-10T08:31:00Z",
            },
            {
              step_index: 1,
              name: "verify",
              phase: "verify",
              status: "running",
              exit_code: null,
              started_at: "2026-11-10T08:31:00Z",
              finished_at: null,
            },
          ],
        },
      ],
    },
    {
      id: "job-linux",
      target: "linux",
      status: "queued",
      target_config: {
        os: "linux",
        arch: "amd64",
        application_platform: "linux",
        steps: [{ name: "compile", phase: "build" }],
      },
      attempts: [],
    },
  ],
  artifacts: [artifact],
  config_yaml:
    "version: 1\n# 原始配置，不翻译\ntargets:\n  macos:\n    os: darwin\n",
};
export const nodes: Node[] = [
  {
    id: "node-main",
    name: "Mac Studio",
    os: "darwin",
    arch: "arm64",
    pool_id: "pool-main",
    pool_name: "default",
    labels: { team: "desktop" },
    capabilities: ["xcode", "codesign", "swift", "git", "node"],
    signing_profiles: ["apple-internal"],
    slots: 2,
    active_jobs: 1,
    draining: false,
    revoked: false,
    online: true,
    last_seen: created,
    certificate_expires_at: "2027-11-10T08:30:00Z",
    diagnostics: {
      emissary_version: "0.1.0",
      buffered_log_bytes: 1024,
      metrics: {
        logical_cpus: 12,
        managed_free_bytes: 73014444032,
        emissary_rss_bytes: 2500000,
      },
      tools: {
        xcode: {
          available: true,
          version: "Xcode 26.0",
          path: "/Applications/Xcode.app",
        },
      },
    },
  },
  {
    id: "node-linux",
    name: "Linux Runner",
    os: "linux",
    arch: "amd64",
    pool_id: "pool-main",
    pool_name: "default",
    labels: {},
    capabilities: ["gcc", "go", "rust", "git"],
    signing_profiles: [],
    slots: 2,
    active_jobs: 0,
    draining: false,
    revoked: false,
    online: false,
    last_seen: created,
    certificate_expires_at: "2027-11-10T08:30:00Z",
    diagnostics: {
      emissary_version: "0.1.0",
      metrics: { logical_cpus: 8, managed_free_bytes: 42949672960 },
    },
  },
];
export const nodeTelemetry: NodeTelemetry = {
  host: {
    node_id: nodes[0].id,
    session_id: "session-main",
    sampled_at: new Date().toISOString(),
    received_at: new Date().toISOString(),
    fresh: true,
    dropped_batches: 0,
    sample: {
      cpu_percent: 24.5,
      memory_total_bytes: 68719476736,
      memory_used_bytes: 17179869184,
      available_memory_bytes: 51539607552,
      build_disk_available_bytes: 73014444032,
      swap_total_bytes: 2147483648,
      swap_used_bytes: 0,
      load_one: 1.25,
      active_attempts: 1,
      disks: [
        {
          device: "disk3s1",
          mount: "/Volumes/Build",
          total_bytes: 1099511627776,
          available_bytes: 73014444032,
          read_bytes: 123456789,
          written_bytes: 987654321,
        },
      ],
      networks: [
        {
          interface: "en0",
          received_bytes: 12345678,
          transmitted_bytes: 23456789,
          receive_errors: 0,
          transmit_errors: 0,
        },
      ],
      accelerators: [
        {
          id: "gpu-0",
          utilization_percent: 12.5,
          memory_used_bytes: 1073741824,
        },
      ],
    },
  },
  inventory: {
    sampled_at: created,
    digest: "inventory-main",
    inventory: {
      os: "darwin",
      arch: "arm64",
      os_version: "macOS 26.1",
      kernel_version: "Darwin 25.1",
      cpu_model: "Apple M4 Max",
      physical_cores: 12,
      logical_cpus: 12,
      physical_memory_bytes: 68719476736,
      effective_cpu_limit: 12,
      effective_memory_limit_bytes: 68719476736,
      container: null,
      virtualization: null,
      tools: {
        xcode: { available: true, version: "Xcode 26.0" },
        cargo: { available: false, version: null },
      },
      capabilities: nodes[0].capabilities,
      signing_profile_refs: ["apple-internal"],
      collectors: { host: "sysinfo" },
      accelerators: [
        {
          id: "gpu-0",
          vendor: "Apple",
          model: "Apple GPU",
          driver: null,
          memory_total_bytes: 68719476736,
        },
      ],
    },
  },
};
export const release: Release = {
  project_slug: project.slug,
  id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  project_id: project.id,
  project_name: project.name,
  version: "1.2.0-internal",
  status: "draft",
  notes: "桌面客户端内部版本\n交付说明与构建要求独立",
  build_count: 2,
  selected_build_id: null,
  delivery_manifest: null,
  publish_error: null,
  created_at: created,
  artifact_count: 1,
  artifacts: [artifact],
};

export function dashboardFixture({ days = 30, timezone = "UTC", projectId }: {
  days?: number;
  timezone?: string;
  projectId?: string | null;
} = {}): DashboardData {
  const end = "2026-11-24T12:00:00Z";
  const start = new Date(Date.UTC(2026, 10, 25 - days)).toISOString();
  const visible = !projectId || projectId === project.id;
  const summary = {
    total_projects: visible ? 1 : 0,
    active_projects: visible ? 1 : 0,
    builds_created: visible ? 1248 : 0,
    succeeded: visible ? 1180 : 0,
    failed: visible ? 40 : 0,
    canceled: visible ? 20 : 0,
    ongoing: visible ? 8 : 0,
    current_ongoing: visible ? 14 : 0,
    success_rate: visible ? 1180 / 1220 : null,
    median_turnaround_seconds: visible ? 480 : null,
  };
  const recent: DashboardBuild[] = visible ? [
    { id: build.id, number: build.number, status: "running", created_at: "2026-11-24T11:00:00Z", finished_at: null, activity_at: "2026-11-24T11:00:00Z" },
    { id: "build-complete", number: 127, status: "succeeded", created_at: "2026-11-24T10:00:00Z", finished_at: "2026-11-24T10:08:00Z", activity_at: "2026-11-24T10:08:00Z" },
    { id: "build-failed", number: 126, status: "failed", created_at: "2026-11-24T09:00:00Z", finished_at: "2026-11-24T09:12:00Z", activity_at: "2026-11-24T09:12:00Z" },
  ].map(value => ({ ...value, project_id: project.id, project_name: project.name, project_slug: project.slug,
    release_id: release.id, release_version: release.version })) : [];
  const share = (total: number, index: number) => Math.floor(total / days) + (index < total % days ? 1 : 0);
  return {
    window: { days, timezone, start, end, generated_at: end },
    summary,
    daily: Array.from({ length: days }, (_, index) => {
      const succeeded = share(summary.succeeded, index);
      const failed = share(summary.failed, index);
      const canceled = share(summary.canceled, index);
      const ongoing = share(summary.ongoing, index);
      return {
        date: new Date(Date.UTC(2026, 10, 25 - days + index)).toISOString().slice(0, 10),
        total: succeeded + failed + canceled + ongoing, succeeded, failed, canceled, ongoing,
        median_turnaround_seconds: succeeded ? 240 + index * 15 : null,
      };
    }),
    recent_projects: recent.length ? [{ id: project.id, name: project.name, slug: project.slug,
      status: project.status, activity_at: recent[0].activity_at, last_build: recent[0] }] : [],
    recent_builds: recent,
  };
}

export async function fixtureAPI(page: Page, role: User["role"] = "admin") {
  const events = { streams: 0, writes: [] as string[] };
  await page.route("**/api/v1/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname.slice("/api/v1".length);
    if (
      !/^\/(auth|tenants|invitations|operations|notifications)(\/|$)/.test(path) &&
      !request.headers()["x-artifactory-tenant"]
    )
      return route.fulfill({
        status: 400,
        json: { error: "tenant context required", code: "tenant_required" },
      });
    if (request.method() !== "GET") {
      events.writes.push(`${request.method()} ${path}`);
      return route.fulfill({
        status: 403,
        json: { error: "UI fixtures do not accept writes" },
      });
    }
    if (path === "/notifications/events") return route.fulfill({contentType:"text/event-stream",body:"retry: 600000\n\n"});
    if (path === "/dashboard") return route.fulfill({ json: dashboardFixture({
      days: Number(url.searchParams.get("days") || 30),
      timezone: url.searchParams.get("timezone") || "UTC",
      projectId: url.searchParams.get("project_id"),
    }) });
    if (path.endsWith("/events")) {
      events.streams++;
      return route.fulfill({
        contentType: "text/event-stream",
        body: `retry: 600000\nevent: snapshot\ndata: ${JSON.stringify(build)}\n\nevent: log\ndata: ${JSON.stringify({ id: 1, attempt_id: "attempt-main", step: "compile", text: "native compilation complete\n原始构建日志\n" })}\n\n`,
      });
    }
    const responses: Record<string, unknown> = {
      "/auth/config": { registration_enabled: true },
      "/auth/me": { id: "user-main", username: "alex", display_name: "Alex" },
      "/notifications": {items:[],next_cursor:null,watermark:0,unread_count:0,needs_action_count:0},
      "/notifications/scopes": {tenants:[{id:tenantId,name:"Build Team",slug:tenantSlug}],projects:[{id:project.id,name:project.name,slug:project.slug,tenant_id:tenantId}]},
      "/notifications/summary": {watermark:0,unread_count:0,needs_action_count:0},
      "/notifications/preferences": {in_app:{},email:{},locale:"zh-CN",email_address:null,email_verified:false,email_available:true},
      "/tenant/notification-webhooks": [],
      [`/projects/${project.id}/notification-webhooks`]: [],
      [`/projects/${project.id}/notification-subscription`]: {following:false},
      "/operations": [],
      "/tenant/lifecycle": {
        projects: 1,
        builds: 1234,
        artifacts: 5,
        devices: 2,
      },
      [`/projects/${project.id}/lifecycle`]: {
        projects: 1,
        builds: 1234,
        artifacts: 5,
        devices: 0,
      },
      "/tenants": [
        {
          id: tenantId,
          slug: tenantSlug,
          name: "Build Team",
          role: role === "admin" ? "owner" : "member",
          owner_id: "user-main",
          created_at: created,
          status: "active",
          archived_at: null,
        },
      ],
      [`/tenants/${tenantSlug}`]: {
        id: tenantId,
        slug: tenantSlug,
        name: "Build Team",
        role: role === "admin" ? "owner" : "member",
        owner_id: "user-main",
        created_at: created,
        status: "active",
        archived_at: null,
      },
      "/tenant": {
        tenant: {
          id: tenantId,
          slug: tenantSlug,
          name: "Build Team",
          role: role === "admin" ? "owner" : "member",
          owner_id: "user-main",
          created_at: created,
          status: "active",
          archived_at: null,
        },
        project_roles: { [project.id]: role },
      },
      "/tenant/members": [
        {
          id: "user-main",
          user_id: "user-main",
          username: "alex",
          role: "owner",
          created_at: created,
        },
      ],
      "/tenant/invitations": [],
      [`/projects/${project.id}/members`]: [],
      [`/projects/${project.id}/member-options`]: [],
      "/projects": [{ ...project, effective_role: role }],
      [`/projects/by-slug/${project.slug}`]: {
        ...project,
        effective_role: role,
      },
      [`/projects/${project.id}`]: { ...project, effective_role: role },
      [`/projects/${project.id}/refs`]: repositoryRefs,
      [`/projects/${project.id}/build-platforms`]: [
        {os:"linux",arch:"amd64",device_count:1,online_count:1},
        {os:"darwin",arch:"arm64",device_count:1,online_count:0},
      ],
      [`/projects/${project.id}/signing-preview`]: [],
      [`/projects/${project.id}/image-credentials`]: [],
      "/builds": [
        build,
        {
          ...build,
          id: "build-complete",
          number: 127,
          status: "succeeded",
          publish_status: "succeeded",
          finished_at: "2026-11-10T08:32:30Z",
          release_version: release.version,
        },
      ],
      [`/projects/by-slug/${project.slug}/builds/${build.number}`]: build,
      [`/builds/${build.id}`]: build,
      "/nodes": nodes,
      [`/nodes/${nodes[0].id}/telemetry`]: {
        ...nodeTelemetry,
        host: {
          ...nodeTelemetry.host,
          sampled_at: new Date().toISOString(),
          received_at: new Date().toISOString(),
        },
      },
      [`/nodes/${nodes[1].id}/telemetry`]: { host: null, inventory: null },
      [`/nodes/${nodes[0].id}/metadata`]: {
        node_id: nodes[0].id,
        startup_id: "startup-main",
        digest: "a".repeat(64),
        received_at: created,
        metadata: {
          "custom.site": "lab",
          "tools.cargo": { available: false, version: null },
        },
      },
      [`/nodes/${nodes[1].id}/metadata`]: {
        startup_id: null,
        digest: null,
        received_at: null,
        metadata: {},
      },
      "/emissary-releases": [],
      "/emissary-updates": [],
      "/overview": {
        projects: 1,
        nodes_online: 1,
        running: 1,
        queued: 2,
        releases: 12,
        builds_today: 42,
      },
      "/releases": [release],
      [`/releases/${release.id}`]: release,
      [`/releases/${release.id}/builds`]: [build, {...build,id:"build-complete",number:127,status:"succeeded",finished_at:created}],
      "/artifacts": [artifact],
      "/images": [],
      "/pools": [{ id: "pool-main", name: "default", is_default: true }],
      "/credentials": [
        {
          id: "credential-main",
          name: "Read-only deploy",
          kind: "https",
          created_at: created,
        },
      ],
      "/tokens": [
        {
          id: "token-main",
          name: "Desktop release",
          scopes: ["read", "build"],
          project_ids: [project.id],
          expires_at: "2027-11-10T08:30:00Z",
          revoked: false,
        },
      ],
      "/model-options": [],
      "/model-profiles": [],
      [`/builds/${build.id}/plans`]: { plans: [], runs: [], events: [] },
      "/users": [{ id: "user-main", username: "alex", role }],
      "/audit": [
        {
          id: 1,
          actor: "user-main",
          action: "project.created",
          resource_id: project.id,
          created_at: created,
        },
      ],
    };
    return Object.hasOwn(responses, path)
      ? route.fulfill({ json: responses[path] })
      : route.fulfill({
          status: 404,
          json: { error: `No fixture for ${path}` },
        });
  });
  return events;
}

export async function openPreferences(page: Page) {
  const personal = page.locator(".account-trigger");
  if (new URL(page.url()).pathname.startsWith("/tenants"))
    await personal.waitFor({state: "visible"});
  if (await personal.isVisible()) {
    await personal.click();
    await page
      .getByRole("menuitem", { name: /^(个人设置|Personal settings)$/ })
      .click();
    await page
      .getByRole("navigation", { name: /^(个人设置分区|Personal settings sections)$/ })
      .getByRole("button", { name: /^(偏好|Preferences)$/, exact: true })
      .click();
    return;
  }
  const trigger = page.getByRole("button", {
    name: /^(界面偏好|Interface preferences)$/,
  });
  if (await trigger.isVisible()) await trigger.click();
}

export async function closePreferences(page: Page) {
  if (await page.getByRole("dialog").isVisible())
    await page.keyboard.press("Escape");
}

export async function openNavigation(page: Page) {
  const trigger = page.getByRole("button", {
    name: /^(打开导航|Open navigation)$/,
    exact: true,
  });
  if (await trigger.isVisible() && await trigger.getAttribute("aria-expanded") !== "true")
    await trigger.click();
}
