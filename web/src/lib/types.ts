/**
 * [INPUT]: 依赖控制平面公开 JSON 契约。
 * [OUTPUT]: 提供Release、可空交付与清单冲突诊断、采样裁剪与规划审计类型。
 * [POS]: API 数据与组件的类型边界；不定义私钥响应。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export interface Account {
  id: string;
  username: string;
  display_name: string;
}
export type WorkspaceStatus = "active" | "draining" | "archived" | "deleting";
export interface LifecycleOperation {
  id: string;
  tenant_id: string;
  project_id: string | null;
  slug: string;
  action: "archive" | "restore" | "delete";
  state: "waiting" | "running" | "succeeded" | "failed" | "canceled";
  phase: string;
  blockers: (
    | {
        kind: "build";
        id: string;
        status: string;
        tenant_slug: string;
        project_slug: string;
        number: number;
      }
    | {
        kind:
          | "publication"
          | "planning"
          | "attempt"
          | "device_update"
          | "source_refs"
          | "release_import";
        id: string;
        status: string;
      }
  )[];
  counts: Record<string, number>;
  error: string | null;
  created_at: string;
  updated_at: string;
}
export type TenantRole = "owner" | "admin" | "member";
export type ProjectRole = "admin" | "operator" | "viewer";
export interface Tenant {
  id: string;
  slug: string;
  name: string;
  role: TenantRole;
  owner_id: string;
  status: WorkspaceStatus;
  archived_at: string | null;
  created_at: string;
}
export interface TenantContext {
  tenant: Tenant;
  project_roles: Record<string, ProjectRole>;
}
export interface TenantMember {
  id: string;
  user_id: string;
  username: string;
  role: TenantRole;
  created_at: string;
}
export interface ProjectMember {
  user_id: string;
  username: string;
  role: ProjectRole;
}
export interface Invitation {
  id: string;
  role: "admin" | "member";
  expires_at: string;
  revoked_at: string | null;
  accepted_at: string | null;
}
export interface NodeAccess {
  mode: "pools" | "nodes";
  ids: string[];
}
export function projectRole(user: User, id: string): ProjectRole {
  return user.project_roles?.[id] || "viewer";
}
export interface User {
  id: string;
  username: string;
  role: "admin" | "operator" | "viewer";
  tenant_role?: TenantRole;
  project_roles?: Record<string, ProjectRole>;
}
export interface AuthConfig {
  registration_enabled: boolean;
}
export interface Project {
  status: WorkspaceStatus;
  archived_at: string | null;
  node_ids?: string[];
  tenant_id?: string;
  node_access?: NodeAccess;
  effective_role?: ProjectRole;
  model_profile_id?: string | null;
  build_context?: string;
  source_credentials?: { repository_url: string; credential_id: string }[];
  id: string;
  slug: string;
  name: string;
  repository_url: string;
  credential_id: string | null;
  pool_ids: string[];
  signing_profiles: string[];
  signing_defaults?: Record<string, string>;
  build_count: number;
  last_build: {
    id: string;
    number: number;
    status: string;
    created_at: string;
  } | null;
}
export interface StepRun {
  step_index: number;
  name: string;
  phase: string;
  status: string;
  exit_code: number | null;
  started_at: string;
  finished_at: string | null;
}
export interface Attempt {
  id: string;
  plan_revision_id?: string | null;
  node_id: string;
  node_name: string;
  status: string;
  lease_expires_at: string;
  error: string | null;
  started_at: string | null;
  finished_at: string | null;
  steps: StepRun[];
}
export type ApplicationPlatform = "macos" | "ios" | "android" | "linux" | "windows";
export interface TargetPlatform {
  os: string;
  arch: string;
  application_platform: ApplicationPlatform | null;
}
export interface Job {
  plan_revision_id?: string | null;
  blocked_reason?: string | null;
  placement?: {
    reason: string | null;
    selected_node: string | null;
    selected_reason: string | null;
    placement: { candidates: { node_id: string; reason: string }[] };
  } | null;
  id: string;
  target: string;
  status: string;
  target_config: TargetPlatform & {
    executor?: "native" | "buildkit";
    steps: { name: string; phase: string }[];
  };
  attempts: Attempt[];
}
export interface Artifact {
  release_id?: string;
  release_version?: string;
  delivery_filename?: string | null;
  signing?: SigningEvidence | null;
  id: string;
  path: string;
  size: number;
  sha256: string;
  target: string;
  created_at: string;
  project_name?: string;
  build_id?: string;
  attempt_status?: string;
}
export interface ImageCredential { id: string; name: string; kind: "registry" | "secret"; enabled: boolean }
export interface ImageExecution {
  parent_id: string | null; tags: string[];
  id: string; job_id: string; operation: "build" | "publish"; status: string;
  spec: { name: string; platforms: string[] }; root_digest: string | null;
  destination_repository: string; destination_credential: string | null; destination_registry?: string | null; error: string | null;
}
export interface Build {
  target_requirements: BuildTarget[];
  selected_targets: string[];
  release_id: string;
  number: number;
  project_slug: string;
  goal?: string | null;
  source_manifest_id?: string | null;
  source_preparation_version?: number;
  current_plan_id?: string | null;
  revision_count?: number;
  clarification?: { question: string } | null;
  id: string;
  project_id: string;
  project_name: string;
  requested_ref: string;
  commit_sha: string | null;
  status: string;
  publish_status: string;
  publish_error: string | null;
  error: string | null;
  release_version: string;
  created_at: string;
  finished_at: string | null;
  jobs?: Job[];
  targets?: { id: string; target: string; status: string }[];
  image_executions?: ImageExecution[];
  image_registry?: string;
  artifacts?: Artifact[];
  config_yaml?: string;
  config_snapshot?: Record<string, unknown>;
  publish_attempts?: { id: string; status: string; error: string | null }[];
}
export interface NodeTool {
  available?: boolean | null;
  path?: string | null;
  version?: string | null;
  [key: string]: unknown;
}
export interface Node {
  id: string;
  name: string;
  identity?: {
    hostname?: string;
    device_model?: string;
    device_vendor?: string;
    cpu_model?: string;
    memory_total_bytes?: number;
  };
  os: string;
  arch: string;
  pool_id: string;
  pool_name: string;
  labels: Record<string, string>;
  capabilities: string[];
  signing_profiles: string[];
  slots: number;
  active_jobs: number;
  draining: boolean;
  revoked: boolean;
  online: boolean;
  last_seen: string | null;
  created_at?: string | null;
  certificate_expires_at: string;
  emissary_target_release?: string | null;
  emissary_desired_release?: string | null;
  emissary_current_version?: string | null;
  emissary_launcher_version?: string | null;
  emissary_ota_status?: Record<string, unknown> | null;
  emissary_pool_paused?: boolean;
  ota_draining?: boolean;
  diagnostics: {
    hostname?: string;
    network_interfaces?: {
      sampled_at: string;
      interfaces: { name: string; addresses: string[] }[];
      truncated: boolean;
    };
    emissary_version?: string;
    emissary_commit?: string;
    protocol_major?: number;
    configured_signing_profiles?: string[];
    buffered_log_bytes?: number;
    metrics?: {
      telemetry?: NodeTelemetryDelivery;
      logical_cpus?: number;
      emissary_rss_bytes?: number;
      managed_free_bytes?: number;
      [key: string]: unknown;
    };
    tools?: Record<string, NodeTool>;
    [key: string]: unknown;
  };
}
export interface NodeTelemetryDelivery {
  state?: string;
  inventory_state?: string;
  spool_bytes?: number;
  spool_records?: number;
  schema_wait_records?: number | null;
  oldest_age_seconds?: number | null;
  inventory_bytes?: number | null;
  inventory_records?: number | null;
  inventory_schema_wait_records?: number | null;
  inventory_oldest_age_seconds?: number | null;
  inventory_superseded?: number | null;
  inventory_conflict_records?: number | null;
  inventory_conflicts_total?: number | null;
  inventory_last_conflict_at?: string | null;
  dropped_batches?: number;
  fresh_bytes?: number | null;
  fresh_records?: number | null;
  fresh_oldest_age_seconds?: number | null;
  coalesced_batches?: number;
  corrupt_records?: number;
  quota_eviction_batches?: number;
  expired_batches?: number;
  malformed_batches?: number;
  disk_errors?: number;
  collector_duration_ms?: number;
  collector_error?: string;
  last_successful_ack_at?: string;
  last_inventory_ack_at?: string;
  upload_latency_ms?: number;
  backoff_seconds?: number;
  negotiation_state?: string;
  last_rejection_reason?: string;
  last_rejection_at?: string | null;
  bounds_exceeded_batches?: number;
  id_content_conflict_batches?: number;
  identity_retired_batches?: number;
  omitted_host_devices?: number;
  truncated_host_fields?: number;
  fresh_state?: string;
  fresh_last_ack_at?: string | null;
  fresh_last_progress_at?: string | null;
  fresh_upload_latency_ms?: number;
  fresh_backoff_seconds?: number;
  history_state?: string;
  history_last_ack_at?: string | null;
  history_last_progress_at?: string | null;
  history_upload_latency_ms?: number;
  history_backoff_seconds?: number;
  inventory_last_ack_at?: string | null;
  inventory_last_progress_at?: string | null;
  inventory_upload_latency_ms?: number;
  inventory_backoff_seconds?: number;
  disk_state?: string;
  disk_observed_at?: string | null;
  disk_file_bytes?: number | null;
  disk_record_bytes?: number | null;
  disk_allocated_bytes?: number | null;
  disk_block_overhead_bytes?: number | null;
  disk_temporary_bytes?: number | null;
  disk_index_bytes?: number | null;
  disk_corrupt_bytes?: number | null;
  disk_unknown_bytes?: number | null;
  disk_files?: number | null;
}
export interface NodeMetadataSnapshot {
  node_id?: string;
  startup_id: string | null;
  digest?: string | null;
  received_at: string | null;
  metadata: Record<string, unknown>;
}
export interface NodeAccelerator {
  id: string;
  vendor?: string | null;
  model?: string | null;
  driver?: string | null;
  memory_total_bytes?: number | null;
}
export interface NodeInventory {
  os?: string;
  arch?: string;
  emissary_version?: string;
  os_version?: string | null;
  kernel_version?: string | null;
  boot_time?: number | null;
  cpu_model?: string | null;
  logical_cpus?: number | null;
  physical_cores?: number | null;
  physical_memory_bytes?: number | null;
  effective_cpu_limit?: number | null;
  effective_memory_limit_bytes?: number | null;
  container?: string | null;
  virtualization?: string | null;
  accelerators?: NodeAccelerator[];
  tools?: Record<string, NodeTool>;
  capabilities?: string[];
  signing_profile_refs?: string[];
  collectors?: Record<string, string>;
  [key: string]: unknown;
}
export interface NodeHostSample {
  cpu_percent?: number | null;
  memory_total_bytes?: number | null;
  memory_used_bytes?: number | null;
  available_memory_bytes?: number | null;
  build_disk_available_bytes?: number | null;
  swap_total_bytes?: number | null;
  swap_used_bytes?: number | null;
  load_one?: number | null;
  active_attempts?: number | null;
  disks?: {
    device: string;
    mount: string;
    total_bytes?: number | null;
    available_bytes?: number | null;
    read_bytes?: number | null;
    written_bytes?: number | null;
  }[];
  networks?: {
    interface: string;
    received_bytes?: number | null;
    transmitted_bytes?: number | null;
    receive_errors?: number | null;
    transmit_errors?: number | null;
  }[];
  accelerators?: {
    id: string;
    utilization_percent?: number | null;
    memory_used_bytes?: number | null;
  }[];
}
export interface NodeTelemetry {
  host: {
    node_id?: string;
    startup_id?: string | null;
    session_id?: string | null;
    sampled_at?: string | null;
    received_at?: string | null;
    fresh?: boolean;
    dropped_batches?: number | null;
    sample: NodeHostSample;
    [key: string]: unknown;
  } | null;
  inventory: {
    sampled_at?: string | null;
    digest?: string | null;
    inventory: NodeInventory;
  } | null;
}
export interface Release {
  project_slug: string;
  id: string;
  project_id: string;
  project_name: string;
  version: string;
  status: "draft" | "publishing" | "published" | "publish_failed";
  notes: string;
  build_count: number;
  selected_build_id: string | null;
  delivery_manifest: Record<string, unknown> | null;
  publish_error: string | null;
  created_at: string;
  artifact_count?: number;
  artifacts?: Artifact[];
  images?: ReleaseImage[];
  image_registry?: string | null;
}
export type BuildTarget = {
  kind: "native";
  key: string;
  os: "linux" | "darwin" | "windows";
  arch: "amd64" | "arm64";
  application_platform?: ApplicationPlatform | null;
  artifact_key?: string;
  format_suffix?: string;
} | {
  kind: "image";
  key: string;
  platforms: ("linux/amd64" | "linux/arm64")[];
};
export interface ReleaseImage {
  id: string;
  name: string;
  target: string;
  repository: string;
  digest: string;
  execution_id?: string;
  version?: string;
  platforms: { platform: string; digest: string }[];
}
export interface ImageArtifact extends ReleaseImage {
  release_id: string;
  release_version: string;
  project_id: string;
  project_slug: string;
  build_number: number;
  registry?: string | null;
  created_at: string;
}
export interface Pool {
  is_default?: boolean;
  id: string;
  name: string;
  emissary_target_release?: string | null;
  emissary_update_concurrency?: number;
  emissary_updates_paused?: boolean;
}
export interface Credential {
  id: string;
  name: string;
  kind: string;
  created_at: string;
}
export interface Token {
  id: string;
  name: string;
  scopes: string[];
  project_ids: string[];
  expires_at: string;
  revoked: boolean;
}
export interface Audit {
  id: number;
  actor: string;
  action: string;
  resource_id: string;
  created_at: string;
}
export interface Overview {
  nodes_registered?: number;
  projects: number;
  nodes_online: number;
  running: number;
  queued: number;
  releases: number;
  builds_today: number;
}

export interface EmissaryRelease {
  id: string;
  version: string;
  created_at: string;
}
export interface EmissaryVersion {
  product_version: string;
  release_id: string | null;
  release_version: string | null;
  release_status: "ready" | "missing" | "ambiguous";
}
export interface EmissaryUpdate {
  id: string;
  release_id: string;
  node_id: string;
  node_name: string;
  version: string;
  previous_version: string | null;
  status: string;
  error: string | null;
  cancel_requested: boolean;
  finished_at: string | null;
  updated_at: string;
}

export interface ModelProfile {
  id: string;
  name: string;
  api: string;
  base_url: string;
  model: string;
  has_key: boolean;
  enabled: boolean;
  is_default: boolean;
  max_calls: number;
  timeout_seconds: number;
  revision: number;
  parameters: {
    context_window?: number;
    max_tokens?: number;
    thinking_level?: string;
    temperature?: number;
    requests_per_minute?: number;
    compat?: {
      system_role?: "system" | "developer";
      thinking_format?: "none" | "reasoning_effort" | "enable_thinking" | "thinking";
      max_tokens_field?: "max_tokens" | "max_completion_tokens";
    };
  };
}
export interface ImageSpec {
  name: string;
  platforms: string[];
  context?: string;
  dockerfile?: string;
  pool?: string;
  allow_emulation?: boolean;
  build_args?: Record<string, string>;
  secrets?: Record<string, string>;
  pull_credentials?: string[];
  tags?: string[];
}
export interface RecipeStep {
  name: string;
  phase: string;
  run: string;
  shell?: string | null;
  timeout_seconds?: number;
}
export interface RecipeTarget extends TargetPlatform {
  executor?: "native" | "buildkit";
  image?: ImageSpec | null;
  timeout_seconds?: number;
  steps: RecipeStep[];
  artifacts?: string[];
  signing_profiles?: string[];
  signing_outputs?: { path: string; bundle?: string | null }[];
  labels?: Record<string, string>;
  capabilities?: string[];
}
export interface PlanRevision {
  id: string;
  revision: number;
  sha256: string;
  created_at: string;
  contents: {
    summary: string;
    evidence: { path: string; reason: string }[];
    scripts: { name: string; content: string }[];
    recipe: {
      version?: number;
      parameters?: Record<string, string>;
      release?: { enabled: boolean };
      targets: Record<string, RecipeTarget>;
    };
  };
}
export interface SourcePreparation {
  run_id: string;
  generation?: number;
  pod_uid?: string;
  pod_name?: string;
  current_stage?: string;
  current_state?: "started" | "completed";
  ready_at?: string;
  started_at?: string;
  updated_at?: string;
  diagnostic?: { stage: string; code: string };
  events?: {
    execution: { generation: number; uid: string; name: string };
    progress: { sequence: number; stage: string; state: "started" | "completed" };
    created_at: string;
  }[];
}
export interface PlanningHistory {
  plans: PlanRevision[];
  runs: {
    id: string;
    state: string;
    purpose: string;
    calls: number;
    error: string | null;
    usage: { totalTokens?: number; cost?: number };
    runtime?: {
      status: string;
      generation: number;
      calls: number;
      usage: { totalTokens?: number; cost?: number };
      preparation?: SourcePreparation;
    };
    runtime_unavailable?: boolean;
  }[];
  events: {
    id: number;
    kind: string;
    payload: Record<string, unknown>;
    created_at: string;
  }[];
}

export interface SigningMetadata {
  identity: string;
  subject: string;
  fingerprint_sha256: string;
  not_before: number;
  not_after: number;
  team_id?: string | null;
  alias?: string | null;
  purpose?: "application" | "upload" | null;
  notarization_available: boolean;
}
export interface SigningBinding {
  profile: string;
  platform: string;
  credential_id: string;
  version_id: string;
  metadata: SigningMetadata;
  notarize: boolean;
  available?: boolean;
}
export interface SigningEvidence {
  path: string;
  sha256: string;
  profile: string;
  version_id: string;
  fingerprint_sha256: string;
  notarized: boolean;
}

export interface PlanningConversation {
 run_id:string; generation:number; pod_uid:string; status:string; model:string;
 purpose?:string;
 source_digest:string|null; started_at:string; ended_at:string|null;
}
export interface ConversationMessage {sequence:number;kind:string;payload:Record<string,unknown>;created_at:string}
export interface ConversationPage {messages:ConversationMessage[];next_cursor:number|null}
