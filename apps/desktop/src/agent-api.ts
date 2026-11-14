import { invoke } from "@tauri-apps/api/core";

export type AgentScope = "observer" | "project_manage" | "execution_test" | "sensitive_operation";
export type AgentConnectionState = "disconnected" | "starting" | "authenticating" | "authentication_required" | "connected" | "quitting" | "unavailable";
export interface AgentOwner { state: AgentConnectionState; code?: string; }
export interface AgentProject { id: string; name: string; }
export interface AgentGrant { grant_id: string; client_id: string; project_id: string; scopes: AgentScope[]; created_at: number; expires_at: number; revoked: boolean; expired: boolean; }
export type AgentReviewTarget =
  | { kind: "service"; action: string }
  | { kind: "runtime_install"; package_id: string }
  | { kind: "runtime_select"; php: string; node: string }
  | { kind: "environment_apply"; write_env: boolean }
  | { kind: "yaml_save"; byte_count: number }
  | { kind: "database_bind"; instance_id: string }
  | { kind: "database_restore"; backup_id: string }
  | { kind: "domain"; action: "hosts" | "trust" | "cleanup" };
export interface AgentReviewDetail { key: string; value: string | number | boolean | string[]; }
export interface AgentPlan {
  digest: string; project_id: string; project_name: string; project_root: string;
  grant_id: string; client_id: string; tool: string; revision: number; expires_at: number;
  state: "pending" | "approved" | "claimed" | "expired" | "revoked" | "stale";
  effects: string[]; required_scopes: AgentScope[]; target: AgentReviewTarget;
  revalidation_required: boolean; machine_confirmation_required: boolean;
  operation_id?: string; details?: AgentReviewDetail[];
}
export interface AgentReceipt { operation_id: string; client_id: string; project_id: string; state: string; code?: string | null; }
export interface AgentOperation { grant_id: string; receipt: AgentReceipt; }
export interface AgentSnapshot { owner: AgentOwner; projects: AgentProject[]; grants: AgentGrant[]; plans: AgentPlan[]; operations: AgentOperation[]; }
export interface AgentConfig { grant_id: string; credential_path: string; config_path: string; config_snippet: string; }
export interface AgentIssue { project_id: string; scopes: AgentScope[]; ttl_seconds: number; }
export interface AgentIssueResult { grant: AgentGrant; config: AgentConfig; }
export interface AgentOperationRef { grant_id: string; operation_id: string; }
export interface AgentLookup { grant_id: string; idempotency_key: string; }
export interface AgentLookupResult { grant_id: string; receipt: AgentReceipt | null; point_in_time: true; }
export interface AgentApi {
  state(): Promise<AgentSnapshot>;
  connect(): Promise<AgentSnapshot>;
  issue(input: AgentIssue): Promise<AgentIssueResult>;
  revoke(grant_id: string): Promise<AgentSnapshot>;
  config(grant_id: string): Promise<AgentConfig>;
  approve(digest: string): Promise<AgentSnapshot>;
  confirmMachine(digest: string): Promise<AgentSnapshot>;
  status(input: AgentOperationRef): Promise<AgentOperation>;
  cancel(input: AgentOperationRef): Promise<AgentOperation>;
  reconcile(input: AgentLookup): Promise<AgentLookupResult>;
}
const request = <T>(input: object): Promise<T> => invoke<T>("agent_connections", { request: input });
export const agentApi: AgentApi = {
  state: () => request({ action: "state" }),
  connect: () => request({ action: "connect" }),
  issue: input => request({ action: "issue", ...input }),
  revoke: grant_id => request({ action: "revoke", grant_id }),
  config: grant_id => request({ action: "config", grant_id }),
  approve: digest => request({ action: "approve", digest }),
  confirmMachine: digest => request({ action: "confirm_machine", digest }),
  status: input => request({ action: "status", ...input }),
  cancel: input => request({ action: "cancel", ...input }),
  reconcile: input => request({ action: "reconcile", ...input }),
};
