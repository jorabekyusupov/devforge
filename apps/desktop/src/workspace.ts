import { useCallback, useEffect, useRef, useState } from "react";
export interface Project {
  mail?: boolean;
  services?: ServiceSelection;
  php_settings?: PhpSettings;
  id: string;
  name: string;
  path: string;
  php: string;
  node: string;
  revision: number;
  php_constraint: string;
  node_constraint: string;
  environment_names: string[];
  active: boolean;
}
export interface Site {
  project_id: string;
  domain: string;
  https: boolean;
  vite: boolean;
}
export interface Runtime {
  problem?: string | null;
  id: string;
  kind: string;
  version: string;
  variant: string;
  installed: boolean;
}
export interface Binding {
  project_id: string;
  instance_id: string;
  database: string;
  role: string;
  secret_ref: string;
  ready?: boolean;
}
export interface Row {
  navigation?: { favorite: boolean; last_opened_ms: number };
  php_diagnostics?: PhpDiagnostics | null;
  setup_owned?: boolean;
  project: Project;
  site: Site | null;
  development: { queue: boolean };
  state: string;
  serving: {
    preferred_url: string;
    http_url: string;
    https_url: string | null;
    fallback_url: string;
    system_trusted: boolean;
    certificate_active: boolean;
    php_ready: boolean;
    vite_ready: boolean;
  } | null;
  queue_state: string | null;
  database: Binding | null;
  backups?: Backup[];
  database_history?: Binding[];
}
export interface Log {
  project_id: string | null;
  process: string;
  stream: string;
  text: string;
}
export interface ServiceSelection {
  cache: "project" | "file" | "database" | "redis";
  queue: "project" | "sync" | "database" | "redis";
  redis_instance: string | null;
}
export interface Backup {
  id: string;
  engine: string;
  version: string;
  bytes: number;
  created: number;
  project: string;
}
export interface Cache {
  instance: {
    id: string;
    project_id: string;
    package: string;
    port: number;
    ready: boolean;
  };
  state: string;
}
export interface Database {
  instance: {
    id: string;
    package: string;
    port: number;
    ready: boolean;
    engine?: "mysql" | "postgresql";
  };
  state: string;
  users: number;
}
export interface Snapshot {
  starter_constraint?: string;
  projects: Row[];
  runtimes: Runtime[];
  databases: Database[];
  caches?: Cache[];
  logs: Log[];
}
export interface Failure {
  scope: string;
  code: string;
  project_id?: string;
}
export interface PhpSettings {
  memory_limit: string | null;
  upload_mb: number | null;
  post_mb: number | null;
  execution_seconds: number | null;
  timezone: string | null;
}
export interface PhpDiagnostics {
  platform_check_available: boolean;
  platform_requirements: { name: string; version: string; status: string }[];
  version: string;
  sapi: string;
  ini: string;
  extensions: string[];
  pdo_drivers: string[];
  settings: Record<string, string>;
  environment_exists: boolean;
  app_key_present: boolean;
  vendor_present: boolean;
}
export interface PrepareOptions {
  composer: boolean;
  npm: boolean;
  create_env: boolean;
  generate_key: boolean;
  run_scripts: boolean;
  initialize_sqlite: boolean;
  trusted: boolean;
}
export type SetupSource =
  | { kind: "laravel"; version: string }
  | { kind: "git"; repository: string; branch: string | null };
export interface CreateInput {
  source: SetupSource;
  parent: string;
  folder: string;
  name: string;
  php: string;
  node: string;
  domain: string;
  https: boolean;
  options: PrepareOptions;
}
export interface SetupProgress {
  stage: string;
  project_id: string | null;
}
export interface HttpsReview {
  preview: {
    domain: string;
    https_enabled: boolean;
    fingerprint: string;
    system_trusted: boolean;
    hosts_path: string;
    hosts_ready: boolean;
    domains: string[];
    elevation_required: boolean;
    helper_available: boolean;
  };
  ticket: string;
  digest: string;
  applied: boolean;
}
export type InspectionKind =
  "request" | "query" | "job" | "log" | "dump" | "exception";
export interface InspectionEvent {
  sequence: number;
  received_ms: number;
  kind: InspectionKind;
  context: string;
  name: string;
  duration_ms: number;
  data: unknown;
}
export interface InspectionProfile {
  captured_queries: number;
  sql_ms: number;
  duration_ms: number | null;
  peak_memory_bytes: number | null;
  truncated: boolean;
  patterns: {
    sql: string;
    count: number;
    total_ms: number;
    slow: boolean;
    repeated: boolean;
  }[];
}
export interface InspectionView {
  capture: { enabled: boolean; events: InspectionEvent[] };
  companion_installed: boolean;
  php_capability?: {
    loaded: boolean;
    version: string | null;
    mode: string | null;
    php: string;
  } | null;
  profiles: Record<string, InspectionProfile>;
}
export interface MailSummary {
  id: string;
  received_ms: number;
  subject: string;
  from: string;
  to: string;
  bytes: number;
  attachments: number;
}
export interface MailAttachment {
  index: number;
  name: string;
  mime: string;
  bytes: number;
  cid: string | null;
}
export interface MailDetail {
  truncated?: boolean;
  summary: MailSummary;
  text: string;
  html: string;
  attachments: MailAttachment[];
}
export interface MailPart {
  attachment: MailAttachment;
  base64: string;
}
export interface MailView {
  enabled: boolean;
  running: boolean;
  port: number | null;
  inbox: { messages: MailSummary[]; total: number; page: number };
}
export interface LogFilter {
  query: string;
  level: string;
  kind: string;
  since: number;
  page: number;
}
export interface LogPage {
  entries: InspectionEvent[];
  total: number;
  page: number;
}
export interface WorkspaceBackend {
  inbox?(id: string, query: string, page: number): Promise<MailView>;
  message?(id: string, message: string): Promise<MailDetail>;
  part?(id: string, message: string, index: number): Promise<MailPart>;
  logs?(id: string, filter: LogFilter): Promise<LogPage>;
  chooseMailFile?(name: string): Promise<string | null>;

  inspector?(id: string, context?: string): Promise<InspectionView>;
  snapshot(): Promise<{
    snapshot: Snapshot;
    busy: boolean;
    operation?: SetupProgress | null;
  }>;
  execute(command: Command): Promise<Record<string, unknown>>;
  cancel(): Promise<void>;
  browse(): Promise<string | null>;
  chooseBackup?(save: boolean): Promise<string | null>;
  open(
    id: string,
    target: "browser" | "editor" | "terminal" | "folder",
  ): Promise<void>;
}
export type ProjectInput = {
  name: string;
  php: string;
  node: string;
  domain: string;
  https: boolean;
  vite: boolean;
  queue: boolean;
};
export type Command =
  | { action: "mail_configure"; id: string; enabled: boolean }
  | {
      action: "mail_delete";
      id: string;
      message: string | null;
      consent: boolean;
    }
  | {
      action: "mail_save";
      id: string;
      message: string;
      index: number;
      path: string;
    }
  | {
      action: "activate_database_generation";
      id: string;
      instance: string;
      database: string;
      consent: boolean;
    }
  | { action: "unbind_database"; id: string; consent: boolean }
  | { action: "install_redis_client"; id: string; consent: boolean }
  | {
      action: "switch_database";
      id: string;
      instance: string;
      consent: boolean;
    }
  | {
      action: "start_database" | "start_cache" | "stop_cache";
      instance: string;
    }
  | { action: "create_cache"; id: string; package: string }
  | {
      action: "configure_services";
      id: string;
      revision: number;
      selection: ServiceSelection;
    }
  | { action: "create_backup"; id: string }
  | { action: "export_backup"; id: string; backup: string; path: string }
  | { action: "import_backup"; id: string; path: string }
  | { action: "restore_backup"; id: string; backup: string; consent: boolean }
  | { action: "create_project"; input: CreateInput }
  | { action: "prepare_project"; id: string; options: PrepareOptions }
  | {
      action: "php_diagnostics" | "https_preview" | "https_export" | "visit";
      id: string;
    }
  | {
      action: "php_settings";
      id: string;
      revision: number;
      settings: PhpSettings;
    }
  | { action: "favorite"; id: string; favorite: boolean }
  | {
      action: "https_apply";
      id: string;
      ticket: string;
      digest: string;
      consent: boolean;
    }
  | {
      action: "inspector_install" | "inspector_clear" | "inspector_probe";
      id: string;
    }
  | { action: "inspector_capture"; id: string; enabled: boolean }
  | { action: "refresh_runtimes" }
  | ({ action: "import"; path: string } & ProjectInput)
  | ({ action: "configure"; id: string; revision: number } & ProjectInput)
  | { action: "forget"; id: string; revision: number }
  | { action: "start" | "stop" | "restart" | "test_database"; id: string }
  | {
      action: "install" | "remove_runtime" | "create_database";
      package: string;
    }
  | { action: "bind_database"; id: string; instance: string }
  | { action: "stop_database"; instance: string };
export function failure(value: unknown): Failure {
  if (value && typeof value === "object" && "scope" in value && "code" in value)
    return value as Failure;
  return {
    scope: "workspace",
    code:
      value === "desktop_unavailable" ? "desktop_unavailable" : "unexpected",
  };
}
export function useWorkspace(backend?: WorkspaceBackend) {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null),
    [error, setError] = useState<Failure | null>(null),
    [pending, setPending] = useState<Command | null>(null),
    [remoteBusy, setRemoteBusy] = useState(false),
    [result, setResult] = useState<Record<string, unknown> | null>(null);
  const [operation, setOperation] = useState<SetupProgress | null>(null);
  const alive = useRef(true),
    working = useRef(false);
  const refresh = useCallback(async () => {
    if (!backend) {
      setError(failure("desktop_unavailable"));
      return;
    }
    try {
      const value = await backend.snapshot();
      if (alive.current) {
        setSnapshot(value.snapshot);
        setRemoteBusy(value.busy);
        setOperation(value.operation ?? null);
      }
    } catch (e) {
      if (alive.current) setError(failure(e));
    }
  }, [backend]);
  useEffect(() => {
    alive.current = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      await refresh();
      if (alive.current) timer = setTimeout(poll, 1500);
    }
    void poll();
    return () => {
      alive.current = false;
      clearTimeout(timer);
    };
  }, [refresh]);
  async function run(
    command: Command,
    onResult?: (value: Record<string, unknown>) => void,
  ) {
    if (!backend || working.current) return false;
    working.current = true;
    setPending(command);
    setError(null);
    setResult(null);
    try {
      const value = await backend.execute(command);
      if (alive.current) setResult(value);
      await refresh();
      onResult?.(value);
      return true;
    } catch (e) {
      if (alive.current) setError(failure(e));
      await refresh();
      return false;
    } finally {
      working.current = false;
      if (alive.current) setPending(null);
    }
  }
  async function open(
    id: string,
    target: "browser" | "editor" | "terminal" | "folder",
  ) {
    try {
      if (!backend) throw "desktop_unavailable";
      await backend.open(id, target);
    } catch (e) {
      setError(failure(e));
    }
  }
  return {
    snapshot,
    error,
    pending,
    busy: !!pending || remoteBusy,
    result,
    operation,
    refresh,
    run,
    open,
    clearError: () => setError(null),
    browse: async () => {
      try {
        return (await backend?.browse()) ?? null;
      } catch (e) {
        setError(failure(e));
        return null;
      }
    },
    cancel: () => backend?.cancel(),
  };
}
