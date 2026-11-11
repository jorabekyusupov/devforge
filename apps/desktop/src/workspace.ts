import { useCallback, useEffect, useRef, useState } from "react";
export interface Project {
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
}
export interface Row {
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
}
export interface Log {
  project_id: string | null;
  process: string;
  stream: string;
  text: string;
}
export interface Database {
  instance: { id: string; package: string; port: number; ready: boolean };
  state: string;
  users: number;
}
export interface Snapshot {
  projects: Row[];
  runtimes: Runtime[];
  databases: Database[];
  logs: Log[];
}
export interface Failure {
  scope: string;
  code: string;
}
export interface WorkspaceBackend {
  snapshot(): Promise<{ snapshot: Snapshot; busy: boolean }>;
  execute(command: Command): Promise<Record<string, unknown>>;
  cancel(): Promise<void>;
  browse(): Promise<string | null>;
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
  async function run(command: Command) {
    if (!backend || working.current) return false;
    working.current = true;
    setPending(command);
    setError(null);
    setResult(null);
    try {
      const value = await backend.execute(command);
      if (alive.current) setResult(value);
      await refresh();
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
