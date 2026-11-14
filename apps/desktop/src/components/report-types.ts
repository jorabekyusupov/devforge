/** Wire shapes from devforge-workspace reports. These are metadata, not live health. */
export type ReportSupport = "supported" | "unsupported" | "unverified";
export type ReportAvailability =
  "available" | "missing" | "unsupported" | "unverified";
export interface OfflineRequirement {
  role: string;
  package_id: string | null;
  selection_basis: string;
  state: ReportAvailability;
  reason: string;
  version: string | null;
  archive_sha256: string | null;
  superseded_by: string | null;
  verification: string;
}
export interface OfflineReport {
  schema: number;
  project_id: string;
  project_revision: number | null;
  scope: string;
  snapshot_semantics: string;
  catalog_fingerprint: string | null;
  catalog_provenance: string;
  managed_artifacts_available: boolean;
  offline_ready: boolean | null;
  requirements: OfflineRequirement[];
  prerequisites: { code: string; state: ReportAvailability; reason: string }[];
  problems: string[];
  registry_complete: boolean;
  receipt_bytes_read: number;
}
export interface EnvironmentReport {
  schema: number;
  project_id: string;
  project_revision: number | null;
  scope: string;
  snapshot_semantics: string;
  matches_recorded_configuration: boolean | null;
  environment_ready: boolean | null;
  manifest_sha256: string | null;
  comparisons: {
    key: string;
    expected: string | null;
    actual: string | null;
    state: "match" | "mismatch" | "missing" | "not_applicable" | "unverified";
    reason: string;
    next_action: string;
  }[];
  portability: {
    target: "macos-arm64" | "macos-x64" | "windows-x64";
    catalog_support: ReportSupport;
    native_support: ReportSupport;
    scope: string;
    reason: string;
  }[];
  secrets: {
    environment: string;
    reference_alias: string;
    registered_name: boolean;
    availability: ReportSupport;
    reason: string;
  }[];
  extensions: {
    extension: string;
    cached_present: boolean | null;
    current_support: ReportSupport;
    evidence: string;
  }[];
  artifact_evidence: OfflineReport;
  problems: string[];
  file_bytes_read: number;
}
export interface StorageReference {
  kind: string;
  id: string;
}
export interface StorageResource {
  category: string;
  resource_id: string;
  project_id: string | null;
  workspace_id: string | null;
  ownership: string;
  logical_bytes: number | null;
  files: number;
  directories: number;
  measurement_complete: boolean;
  active_refs: StorageReference[];
  reference_coverage: string;
  retained_reason: string;
  problems: string[];
}
export interface StorageInventory {
  schema: number;
  size_semantics: string;
  scope_semantics: string;
  registry_complete: boolean;
  snapshot_semantics: string;
  logical_bytes_observed: number;
  entries_examined: number;
  registry_rows_examined: number;
  registry_bytes_read: number;
  measurement_complete: boolean;
  resources: StorageResource[];
  problems: string[];
}
export interface RuntimeCleanupPreview {
  schema: number;
  action: string;
  package_id: string;
  state: "blocked" | "review_candidate" | "unknown";
  descriptor_sha256: string | null;
  references: StorageReference[];
  lease: {
    state: "held" | "unheld_at_observation" | "missing" | "unknown";
    reason: string;
  };
  logical_bytes: number | null;
  size_semantics: string;
  snapshot_semantics: string;
  apply_semantics: string;
  problems: string[];
}
export interface ReportPresentationState {
  loading: boolean;
  errorCode?: string | null;
  onRefresh?: () => void;
}
