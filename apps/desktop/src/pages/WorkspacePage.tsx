import { ProjectServices } from "../components/ProjectServices";
import { WorkspaceModal as Modal } from "../components/WorkspaceModal";
import {
  CreateProjectDialog,
  PrepareProjectDialog,
  PhpSettingsDialog,
  HttpsSetupDialog,
} from "../components/ProjectSetup";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Folder,
  Plus,
  Search,
  ExternalLink,
  Play,
  Square,
  RotateCw,
  Terminal,
  Code2,
  Trash2,
  Download,
  RefreshCw,
  AlertCircle,
  X,
  Database as DatabaseIcon,
  CheckCircle2,
  Star,
} from "lucide-react";
import {
  useWorkspace,
  type WorkspaceBackend,
  type Row,
  type ProjectInput,
  type Command,
  type Snapshot,
  type Runtime,
  type HttpsReview,
} from "../workspace";
export function WorkspacePage({
  backend,
  mode,
}: {
  backend?: WorkspaceBackend;
  mode: "projects" | "runtimes" | "services";
}) {
  const { t, i18n } = useTranslation();
  const api = useWorkspace(backend);
  const [queries, setQueries] = useState({
      projects: "",
      runtimes: "",
      services: "",
    }),
    [selected, setSelected] = useState(
      () =>
        new URLSearchParams(location.hash.split("?")[1]).get("project") ?? "",
    ),
    [adding, setAdding] = useState(false),
    [creating, setCreating] = useState(false),
    [tool, setTool] = useState<{
      kind: "prepare" | "php" | "https";
      id: string;
    } | null>(null),
    [projectFilter, setProjectFilter] = useState<
      "all" | "recent" | "favorites"
    >("all"),
    [confirm, setConfirm] = useState<Command | null>(null),
    [stream, setStream] = useState("all");
  const query = queries[mode];
  const setQuery = (value: string) =>
    setQueries((current) => ({ ...current, [mode]: value }));
  const projects = [...(api.snapshot?.projects ?? [])].sort((a, b) =>
    a.project.name.localeCompare(b.project.name, i18n.language),
  );
  const row = projects.find((p) => p.project.id === selected) ?? projects[0];
  const runtimes = api.snapshot?.runtimes ?? [];
  useEffect(() => {
    const changed = () =>
      setSelected(
        new URLSearchParams(location.hash.split("?")[1]).get("project") ?? "",
      );
    window.addEventListener("hashchange", changed);
    return () => window.removeEventListener("hashchange", changed);
  }, []);
  const errorKey = api.error
    ? `${api.error.scope}.errors.${api.error.code}`
    : "";
  const errorMessage = api.error
    ? i18n.exists(errorKey)
      ? t(errorKey)
      : api.error.code === "desktop_unavailable"
        ? t("errors.desktop_unavailable")
        : t("errors.unexpected")
    : "";
  const filtered = projects
    .filter((r) =>
      projectFilter === "all" || projectFilter === "favorites"
        ? projectFilter === "all" || !!r.navigation?.favorite
        : !!r.navigation?.last_opened_ms,
    )
    .filter((r) =>
      `${r.project.name} ${r.project.path} ${r.site?.domain ?? ""}`
        .toLocaleLowerCase()
        .includes(query.toLocaleLowerCase()),
    );
  if (projectFilter === "recent")
    filtered.sort(
      (a, b) =>
        (b.navigation?.last_opened_ms ?? 0) -
        (a.navigation?.last_opened_ms ?? 0),
    );
  const toolRow = tool
    ? projects.find((p) => p.project.id === tool.id)
    : undefined;
  const toolError = api.error ? errorMessage : "";
  const logs = (api.snapshot?.logs ?? []).filter(
    (l) =>
      (mode !== "projects" || l.project_id === row?.project.id) &&
      (stream === "all" || l.stream === stream) &&
      `${l.process} ${l.text}`
        .toLocaleLowerCase()
        .includes(mode === "services" ? query.toLocaleLowerCase() : ""),
  );
  return (
    <section
      className="workspace-content"
      data-mode={mode}
      aria-busy={api.busy}
    >
      {api.error && !adding && !creating && !tool && !confirm && (
        <div className="notice error" role="alert">
          <AlertCircle aria-hidden="true" />
          <div>
            <p>{errorMessage}</p>
            <a href="#runtimes">{t("work.checkRuntimes")}</a>
          </div>
          <button onClick={() => void api.refresh()} disabled={api.busy}>
            {t("common.retry")}
          </button>
          <button aria-label={t("work.dismiss")} onClick={api.clearError}>
            <X aria-hidden="true" />
          </button>
        </div>
      )}
      {api.busy && (
        <div role="status" className="operation">
          <span className="spinner" aria-hidden="true" />
          {t(
            api.pending
              ? `work.operations.${api.pending.action}`
              : "work.working",
          )}
          {[
            "install",
            "create_project",
            "prepare_project",
            "create_backup",
            "restore_backup",
            "install_redis_client",
          ].includes(api.pending?.action ?? "") && (
            <button onClick={() => void api.cancel()}>
              {t("work.cancel")}
            </button>
          )}
        </div>
      )}
      {mode === "services" &&
        api.result &&
        api.result.connected === undefined && (
          <p role="status" className="notice">
            {t("services.completed")}
          </p>
        )}
      {api.result?.connected !== undefined && (
        <p role="status" className="notice">
          <CheckCircle2 aria-hidden="true" />
          {t(api.result.connected ? "work.connected" : "work.connectionFailed")}
        </p>
      )}
      {!api.snapshot && !api.error && <p role="status">{t("work.loading")}</p>}
      {mode === "projects" && (
        <>
          <div className="workspace-toolbar">
            <label className="search-field">
              <Search aria-hidden="true" />
              <span className="sr-only">{t("work.searchProjects")}</span>
              <input
                type="search"
                name="project-search"
                autoComplete="off"
                value={query}
                placeholder={t("work.searchProjects")}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
            <button
              onClick={() => {
                api.clearError();
                setCreating(true);
              }}
              className="primary"
              disabled={api.busy || !api.snapshot}
            >
              <Plus aria-hidden="true" />
              {t("setup.newProject")}
            </button>
            <button
              onClick={() => setAdding(true)}
              disabled={api.busy || !api.snapshot}
            >
              <Plus aria-hidden="true" />
              {t("work.addProject")}
            </button>
          </div>
          <div
            className="project-filters"
            role="group"
            aria-label={t("setup.projectFilter")}
          >
            {(["all", "recent", "favorites"] as const).map((key) => (
              <button
                key={key}
                aria-pressed={projectFilter === key}
                onClick={() => setProjectFilter(key)}
              >
                {t(`setup.filters.${key}`)}
              </button>
            ))}
          </div>
          {api.snapshot && !api.busy && projects.length === 0 && (
            <div className="workspace-empty">
              <Folder aria-hidden="true" />
              <h2>{t("work.noProjects")}</h2>
              <p>{t("work.importHint")}</p>
              <button
                className="primary"
                onClick={() => setAdding(true)}
                disabled={api.busy}
              >
                {t("work.addProject")}
              </button>
            </div>
          )}
          {projects.length > 0 && (
            <div className="project-layout">
              <div className="project-list" aria-label={t("work.projectList")}>
                {filtered.map((p) => (
                  <div className="project-list-item" key={p.project.id}>
                    <a
                      key={p.project.id}
                      href={`#projects?project=${encodeURIComponent(p.project.id)}`}
                      className={`project-row ${row?.project.id === p.project.id ? "active" : ""}`}
                      aria-current={
                        row?.project.id === p.project.id ? "true" : undefined
                      }
                      onClick={() => {
                        setSelected(p.project.id);
                        if (!api.busy)
                          void api.run({ action: "visit", id: p.project.id });
                      }}
                    >
                      <div>
                        <Folder aria-hidden="true" />
                        <strong translate="no">{p.project.name}</strong>
                        <StateBadge
                          value={
                            api.pending &&
                            ["start", "restart"].includes(api.pending.action) &&
                            "id" in api.pending &&
                            api.pending.id === p.project.id
                              ? "starting"
                              : p.state
                          }
                        />
                      </div>
                      <span translate="no">{p.site?.domain}</span>
                      <code title={p.project.path} translate="no">
                        {p.project.path}
                      </code>
                    </a>
                    <button
                      className="favorite-button"
                      disabled={api.busy}
                      aria-label={t(
                        p.navigation?.favorite
                          ? "setup.unfavorite"
                          : "setup.favorite",
                        { name: p.project.name },
                      )}
                      aria-pressed={!!p.navigation?.favorite}
                      onClick={() =>
                        void api.run({
                          action: "favorite",
                          id: p.project.id,
                          favorite: !p.navigation?.favorite,
                        })
                      }
                    >
                      <Star
                        aria-hidden="true"
                        fill={p.navigation?.favorite ? "currentColor" : "none"}
                      />
                    </button>
                  </div>
                ))}
                {filtered.length === 0 && (
                  <p className="muted">{t("work.noMatches")}</p>
                )}
              </div>
              {row && (
                <ProjectDetail
                  key={`${row.project.id}-${row.project.revision}`}
                  row={row}
                  snapshot={api.snapshot!}
                  busy={api.busy}
                  run={api.run}
                  open={api.open}
                  onForget={() =>
                    setConfirm({
                      action: "forget",
                      id: row.project.id,
                      revision: row.project.revision,
                    })
                  }
                  onTool={(kind) => {
                    api.clearError();
                    setTool({ kind, id: row.project.id });
                    if (kind === "php")
                      void api.run({
                        action: "php_diagnostics",
                        id: row.project.id,
                      });
                    if (kind === "https")
                      void api.run({
                        action: "https_export",
                        id: row.project.id,
                      });
                  }}
                />
              )}
            </div>
          )}
        </>
      )}
      {mode === "runtimes" && (
        <>
          <div className="workspace-toolbar">
            <label className="search-field">
              <Search aria-hidden="true" />
              <span className="sr-only">{t("work.searchRuntimes")}</span>
              <input
                type="search"
                placeholder={t("work.searchRuntimes")}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
            <button
              disabled={api.busy}
              onClick={() => void api.run({ action: "refresh_runtimes" })}
            >
              <RefreshCw aria-hidden="true" />
              {t("work.refresh")}
            </button>
          </div>
          <div className="table-scroll">
            <table className="runtime-table">
              <caption className="sr-only">{t("runtimes.title")}</caption>
              <thead>
                <tr>
                  <th>{t("work.runtime")}</th>
                  <th>{t("work.version")}</th>
                  <th>{t("work.status")}</th>
                  <th>
                    <span className="sr-only">{t("work.actions")}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {runtimes
                  .filter((p) =>
                    `${p.kind} ${p.version} ${p.variant}`
                      .toLowerCase()
                      .includes(query.toLowerCase()),
                  )
                  .map((p) => (
                    <tr key={p.id}>
                      <td>
                        <strong>{t(`work.kinds.${p.kind}`)}</strong>
                        <small>{p.variant}</small>
                      </td>
                      <td>
                        <code>{p.version}</code>
                        {p.replacement && !p.installed && (
                          <small>{t("work.runtimeSuperseded")}</small>
                        )}
                      </td>
                      <td>
                        <StateBadge
                          value={
                            p.problem
                              ? "damaged"
                              : p.installed
                                ? "installed"
                                : "missing"
                          }
                        />
                      </td>
                      <td>
                        {p.replacement && !p.installed ? (
                          <button
                            disabled={api.busy}
                            onClick={() =>
                              void api.run({
                                action: "install",
                                package: p.replacement!,
                              })
                            }
                          >
                            <Download aria-hidden="true" />
                            {t("work.installReplacement")}
                          </button>
                        ) : p.installed || p.problem ? (
                          <button
                            aria-label={t("work.removeRuntimeLabel", {
                              name: `${p.kind} ${p.version} ${p.variant}`,
                            })}
                            disabled={api.busy}
                            onClick={() =>
                              setConfirm({
                                action: "remove_runtime",
                                package: p.id,
                              })
                            }
                          >
                            <Trash2 aria-hidden="true" />
                            {t("work.remove")}
                          </button>
                        ) : (
                          <button
                            disabled={api.busy}
                            onClick={() =>
                              void api.run({ action: "install", package: p.id })
                            }
                          >
                            <Download aria-hidden="true" />
                            {t("work.install")}
                          </button>
                        )}
                      </td>
                    </tr>
                  ))}
              </tbody>
            </table>
          </div>
        </>
      )}
      {mode === "services" && (
        <>
          <DatabaseServices
            snapshot={api.snapshot}
            busy={api.busy}
            run={api.run}
          />
          <ProjectServices
            snapshot={api.snapshot}
            busy={api.busy}
            run={api.run}
            backend={backend}
          />
          <div className="section-toolbar">
            <h2>{t("work.logs")}</h2>
            <label>
              {t("work.stream")}
              <select
                value={stream}
                onChange={(e) => setStream(e.target.value)}
              >
                <option value="all">{t("work.allStreams")}</option>
                <option value="stdout">stdout</option>
                <option value="stderr">stderr</option>
              </select>
            </label>
            <label className="search-field">
              <Search aria-hidden="true" />
              <span className="sr-only">{t("work.searchLogs")}</span>
              <input
                type="search"
                value={query}
                placeholder={t("work.searchLogs")}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
          </div>
          <LogViewer logs={logs} />
        </>
      )}
      {mode === "projects" && row && (
        <>
          <div className="section-toolbar">
            <h2>{t("work.logs")}</h2>
            <a href={`#services?project=${row.project.id}`}>
              {t("work.allLogs")}
            </a>
          </div>
          <LogViewer logs={logs} />
        </>
      )}
      {adding && (
        <ProjectDialog
          runtimes={runtimes}
          errorText={errorMessage}
          busy={api.busy}
          browse={api.browse}
          onClose={() => setAdding(false)}
          onSubmit={async (value) => {
            if (await api.run(value)) setAdding(false);
          }}
        />
      )}
      {creating && (
        <CreateProjectDialog
          runtimes={runtimes}
          starter={api.snapshot?.starter_constraint ?? ""}
          busy={api.busy}
          error={api.error}
          operation={api.operation}
          browse={api.browse}
          run={api.run}
          cancel={() => void api.cancel()}
          onClose={() => setCreating(false)}
          onCreated={(id) => {
            setCreating(false);
            setSelected(id);
            location.hash = `projects?project=${encodeURIComponent(id)}`;
          }}
        />
      )}
      {toolRow && tool?.kind === "prepare" && (
        <PrepareProjectDialog
          row={toolRow}
          busy={api.busy}
          errorText={toolError}
          operation={api.operation}
          run={api.run}
          cancel={() => void api.cancel()}
          onClose={() => setTool(null)}
        />
      )}
      {toolRow && tool?.kind === "php" && (
        <PhpSettingsDialog
          row={toolRow}
          busy={api.busy}
          errorText={toolError}
          run={api.run}
          onClose={() => setTool(null)}
        />
      )}
      {toolRow && tool?.kind === "https" && (
        <HttpsSetupDialog
          row={toolRow}
          review={
            api.result && "preview" in api.result
              ? (api.result as unknown as HttpsReview)
              : null
          }
          busy={api.busy}
          errorText={toolError}
          run={api.run}
          onClose={() => setTool(null)}
        />
      )}
      {confirm && (
        <ConfirmDialog
          errorText={errorMessage}
          busy={api.busy}
          onClose={() => setConfirm(null)}
          onConfirm={async () => {
            if (await api.run(confirm)) setConfirm(null);
          }}
        />
      )}
    </section>
  );
}
function StateBadge({ value }: { value: string }) {
  const { t } = useTranslation();
  return (
    <span className={`state state-${value}`}>
      <i aria-hidden="true" />
      {t(`work.states.${value}`, { defaultValue: value })}
    </span>
  );
}
function runtimeOptions(list: Runtime[], kind: string) {
  return list.filter(
    (p) => p.kind === kind && (kind !== "php" || p.variant !== "fpm"),
  );
}
function RuntimeSelect({
  kind,
  value,
  onChange,
  list,
}: {
  kind: string;
  value: string;
  onChange: (v: string) => void;
  list: Runtime[];
}) {
  const { t } = useTranslation();
  return (
    <label>
      {t(`work.kinds.${kind}`)}
      <select
        name={kind}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        required
      >
        {runtimeOptions(list, kind).map((p) => (
          <option value={p.id} key={p.id}>
            {p.version}
            {p.installed ? "" : ` · ${t("work.states.missing")}`}
          </option>
        ))}
      </select>
    </label>
  );
}
function ProjectDetail({
  row,
  snapshot,
  busy,
  run,
  open,
  onForget,
  onTool,
}: {
  row: Row;
  snapshot: Snapshot;
  busy: boolean;
  run: (c: Command) => Promise<boolean>;
  open: (
    id: string,
    t: "browser" | "editor" | "terminal" | "folder",
  ) => Promise<void>;
  onForget: () => void;
  onTool: (kind: "prepare" | "php" | "https") => void;
}) {
  const { t } = useTranslation();
  const p = row.project;
  const [form, setForm] = useState<ProjectInput>({
    name: p.name,
    php: p.php,
    node: p.node,
    domain: row.site?.domain ?? "",
    https: row.site?.https ?? true,
    vite: row.site?.vite ?? false,
    queue: row.development.queue,
  });
  const [instance, setInstance] = useState(row.database?.instance_id ?? "");
  const active = !!row.serving;
  const update = <K extends keyof ProjectInput>(
    key: K,
    value: ProjectInput[K],
  ) => setForm((f) => ({ ...f, [key]: value }));
  const [leaveTo, setLeaveTo] = useState<string | null>(null);
  const dirty =
    JSON.stringify(form) !==
    JSON.stringify({
      name: p.name,
      php: p.php,
      node: p.node,
      domain: row.site?.domain ?? "",
      https: row.site?.https ?? true,
      vite: row.site?.vite ?? false,
      queue: row.development.queue,
    });
  useEffect(() => {
    if (!dirty || busy) return;
    const guard = (event: MouseEvent) => {
      const link = (event.target as Element).closest<HTMLAnchorElement>(
        'a[href^="#"]',
      );
      if (
        link &&
        link.hash !== location.hash &&
        !event.metaKey &&
        !event.ctrlKey
      ) {
        event.preventDefault();
        event.stopPropagation();
        setLeaveTo(link.hash);
      }
    };
    const unload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
    };
    document.addEventListener("click", guard, true);
    window.addEventListener("beforeunload", unload);
    return () => {
      document.removeEventListener("click", guard, true);
      window.removeEventListener("beforeunload", unload);
    };
  }, [dirty, busy]);
  return (
    <article className="project-detail">
      <div className="detail-title">
        <Folder aria-hidden="true" />
        <h2 translate="no">{p.name}</h2>
        <StateBadge value={row.state} />
      </div>
      <div className="project-url">
        <code translate="no">
          {row.serving?.preferred_url ?? row.site?.domain}
        </code>
        <button
          disabled={busy || row.state !== "running"}
          onClick={() => void open(p.id, "browser")}
        >
          <ExternalLink aria-hidden="true" />
          {t("work.openBrowser")}
        </button>
      </div>
      <div className="action-bar">
        <button
          disabled={busy || active}
          onClick={() => void run({ action: "start", id: p.id })}
        >
          <Play aria-hidden="true" />
          {t("work.start")}
        </button>
        <button
          disabled={busy || !active}
          onClick={() => void run({ action: "stop", id: p.id })}
        >
          <Square aria-hidden="true" />
          {t("work.stop")}
        </button>
        <button
          disabled={busy || !active}
          onClick={() => void run({ action: "restart", id: p.id })}
        >
          <RotateCw aria-hidden="true" />
          {t("work.restart")}
        </button>
        <button disabled={busy} onClick={() => void open(p.id, "editor")}>
          <Code2 aria-hidden="true" />
          {t("work.openEditor")}
        </button>
        <button disabled={busy} onClick={() => void open(p.id, "terminal")}>
          <Terminal aria-hidden="true" />
          {t("work.openTerminal")}
        </button>
        <button disabled={busy} onClick={() => void open(p.id, "folder")}>
          <Folder aria-hidden="true" />
          {t("work.openFolder")}
        </button>
      </div>
      {row.serving && !row.serving.system_trusted && row.site?.https && (
        <p className="notice">{t("work.trustFallback")}</p>
      )}
      <div className="action-bar setup-tools">
        <button disabled={busy || dirty} onClick={() => onTool("prepare")}>
          {t("setup.prepare")}
        </button>
        <button
          id={`project-${p.id}-php-settings`}
          disabled={busy || dirty}
          onClick={() => onTool("php")}
        >
          {t("setup.phpSettings")}
        </button>
        <button disabled={busy || dirty} onClick={() => onTool("https")}>
          {t("setup.systemHttps")}
        </button>
      </div>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void run({
            action: "configure",
            id: p.id,
            revision: p.revision,
            ...form,
          });
        }}
      >
        <fieldset disabled={busy}>
          <div className="detail-grid">
            <section className="work-panel">
              <h3>{t("work.runtime")}</h3>
              <p className="muted">{t("work.runtimeHint")}</p>
              <RuntimeSelect
                kind="php"
                value={form.php}
                onChange={(v) => update("php", v)}
                list={snapshot.runtimes}
              />
              <RuntimeSelect
                kind="node"
                value={form.node}
                onChange={(v) => update("node", v)}
                list={snapshot.runtimes}
              />
              <p className="constraints" translate="no">
                PHP {p.php_constraint} · Node {p.node_constraint}
              </p>
            </section>
            <section className="work-panel">
              <h3>{t("work.development")}</h3>
              <label className="toggle-row">
                <span>{t("work.vite")}</span>
                <input
                  type="checkbox"
                  role="switch"
                  checked={form.vite}
                  onChange={(e) => update("vite", e.target.checked)}
                />
              </label>
              <label className="toggle-row">
                <span>{t("work.queue")}</span>
                <input
                  type="checkbox"
                  role="switch"
                  checked={form.queue}
                  onChange={(e) => update("queue", e.target.checked)}
                />
              </label>
              {row.queue_state && (
                <p>
                  {t("work.queue")}: <StateBadge value={row.queue_state} />
                </p>
              )}
            </section>
          </div>
          <details className="advanced-config">
            <summary>{t("work.domainSettings")}</summary>
            <label className="toggle-row">
              <span>HTTPS</span>
              <input
                type="checkbox"
                role="switch"
                checked={form.https}
                onChange={(e) => update("https", e.target.checked)}
              />
            </label>
            <label>
              {t("work.domain")}
              <input
                name="domain"
                value={form.domain}
                required
                pattern="[a-z0-9][a-z0-9.\-]*\.devforge\.test"
                spellCheck={false}
                onChange={(e) => update("domain", e.target.value)}
              />
            </label>
          </details>
          <div className="save-row">
            <p className="muted">{t("work.saveHint")}</p>
            <button className="primary" type="submit">
              {t("work.save")}
            </button>
          </div>
        </fieldset>
      </form>
      <section className="work-panel database-panel">
        <h3>{t("work.database")}</h3>
        <p className="muted">
          {row.database ? (
            <code translate="no">{row.database.database}</code>
          ) : (
            t("work.noDatabase")
          )}
        </p>
        <div className="database-controls">
          <label>
            {t("work.databaseService")}
            <select
              value={instance}
              disabled={busy || active}
              onChange={(e) => setInstance(e.target.value)}
            >
              <option value="">{t("work.selectDatabase")}</option>
              {snapshot.databases.map((d) => (
                <option key={d.instance.id} value={d.instance.id}>
                  {d.instance.engine === "mysql" ? "MySQL" : "PostgreSQL"} ·{" "}
                  {d.instance.port}
                </option>
              ))}
            </select>
          </label>
          <button
            disabled={busy || active || !instance || !!row.database}
            onClick={() =>
              void run({ action: "bind_database", id: p.id, instance })
            }
          >
            {t("work.bindDatabase")}
          </button>
          <button
            disabled={busy || !row.database}
            onClick={() => void run({ action: "test_database", id: p.id })}
          >
            {t("work.testConnection")}
          </button>
        </div>
        <a href="#services">{t("work.manageDatabases")}</a>
      </section>
      <details className="project-meta">
        <summary>{t("work.projectDetails")}</summary>
        <dl>
          <dt>{t("work.path")}</dt>
          <dd>
            <code translate="no">{p.path}</code>
          </dd>
          <dt>{t("work.domain")}</dt>
          <dd translate="no">{row.site?.domain}</dd>
        </dl>
        <button className="danger" disabled={busy} onClick={onForget}>
          <Trash2 aria-hidden="true" />
          {t("work.forget")}
        </button>
        <p className="muted">{t("work.dataPreserved")}</p>
      </details>
      {leaveTo && (
        <Modal
          title={t("work.unsaved")}
          busy={false}
          onClose={() => setLeaveTo(null)}
        >
          <p>{t("work.unsavedHint")}</p>
          <div className="modal-actions">
            <button onClick={() => setLeaveTo(null)} data-initial-focus>
              {t("work.keepEditing")}
            </button>
            <button
              className="danger"
              onClick={() => {
                const target = leaveTo;
                setLeaveTo(null);
                location.hash = target;
              }}
            >
              {t("work.discard")}
            </button>
          </div>
        </Modal>
      )}
    </article>
  );
}
function DatabaseServices({
  snapshot,
  busy,
  run,
}: {
  snapshot: Snapshot | null;
  busy: boolean;
  run: (c: Command) => Promise<boolean>;
}) {
  const { t } = useTranslation();
  const list =
    snapshot?.runtimes
      .filter(
        (r) =>
          (r.kind === "postgresql" || r.kind === "mysql") &&
          r.variant === "server",
      )
      .sort(
        (a, b) =>
          a.kind.localeCompare(b.kind) ||
          new Intl.Collator("en", { numeric: true }).compare(
            b.version,
            a.version,
          ),
      ) ?? [];
  const [selected, setSelected] = useState("");
  const value = selected || list[0]?.id || "";
  return (
    <section className="work-panel">
      <div className="section-toolbar">
        <h2>
          <DatabaseIcon aria-hidden="true" />
          {t("work.databaseServices")}
        </h2>
        <label>
          {t("work.version")}
          <select
            value={value}
            disabled={busy}
            onChange={(e) => setSelected(e.target.value)}
          >
            {list.map((p) => (
              <option key={p.id} value={p.id}>
                {p.kind === "mysql" ? "MySQL" : "PostgreSQL"} · {p.version}
                {p.installed ? "" : ` · ${t("work.states.missing")}`}
              </option>
            ))}
          </select>
        </label>
        <button
          className="primary"
          disabled={busy || !list.find((p) => p.id === value)?.installed}
          onClick={() =>
            void run({ action: "create_database", package: value })
          }
        >
          <Plus aria-hidden="true" />
          {t("work.createDatabase")}
        </button>
      </div>
      <p className="muted">{t("work.databaseHint")}</p>
      {!snapshot?.databases.length && (
        <p className="empty-copy">{t("work.noServices")}</p>
      )}
      {snapshot?.databases.map((d) => (
        <div className="service-row" key={d.instance.id}>
          <div>
            <strong>
              {d.instance.engine === "mysql" ? "MySQL" : "PostgreSQL"} ·{" "}
              {
                snapshot.runtimes.find((r) => r.id === d.instance.package)
                  ?.version
              }{" "}
              · {d.instance.port}
            </strong>
            <p>{t("work.projectUsers", { count: d.users })}</p>
          </div>
          <StateBadge value={d.state} />
          <button
            disabled={busy || d.users > 0 || !d.instance.ready}
            onClick={() =>
              void run({
                action:
                  d.state === "stopped" ? "start_database" : "stop_database",
                instance: d.instance.id,
              })
            }
          >
            {t(d.state === "stopped" ? "work.start" : "work.stop")}
          </button>
        </div>
      ))}
    </section>
  );
}
function LogViewer({ logs }: { logs: Snapshot["logs"] }) {
  const { t } = useTranslation();
  return (
    <div className="log-viewer" tabIndex={0} aria-label={t("work.logs")}>
      {logs.length ? (
        logs.map((log, i) => (
          <div key={`${log.process}-${i}`} className="log-line">
            <code>{log.process}</code>
            <span>{log.stream}</span>
            <pre translate="no">{log.text}</pre>
          </div>
        ))
      ) : (
        <p>{t("work.noLogs")}</p>
      )}
    </div>
  );
}
function ProjectDialog({
  runtimes,
  errorText,
  busy,
  browse,
  onClose,
  onSubmit,
}: {
  runtimes: Runtime[];
  errorText: string;
  busy: boolean;
  browse: () => Promise<string | null>;
  onClose: () => void;
  onSubmit: (v: Extract<Command, { action: "import" }>) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [path, setPath] = useState("");
  const [form, setForm] = useState<ProjectInput>({
    name: "",
    php: runtimeOptions(runtimes, "php")[0]?.id ?? "",
    node: runtimeOptions(runtimes, "node")[0]?.id ?? "",
    domain: "",
    https: true,
    vite: false,
    queue: false,
  });
  const update = <K extends keyof ProjectInput>(
    key: K,
    value: ProjectInput[K],
  ) => setForm((f) => ({ ...f, [key]: value }));
  return (
    <Modal title={t("work.addProject")} busy={busy} onClose={onClose}>
      {errorText && (
        <p role="alert" className="modal-error">
          {errorText}
        </p>
      )}
      <form
        onInvalidCapture={(e) =>
          (e.target as HTMLInputElement).setCustomValidity(
            t("work.invalidForm"),
          )
        }
        onInputCapture={(e) =>
          (e.target as HTMLInputElement).setCustomValidity("")
        }
        onSubmit={(e) => {
          e.preventDefault();
          void onSubmit({ action: "import", path, ...form });
        }}
      >
        <fieldset disabled={busy}>
          <label htmlFor="project-import-path">{t("work.path")}</label>
          <div className="input-action">
            <input
              id="project-import-path"
              name="path"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              required
              autoComplete="off"
              spellCheck={false}
              autoFocus
            />
            <button
              type="button"
              onClick={async () => {
                const p = await browse();
                if (p) setPath(p);
              }}
            >
              {t("work.browse")}
            </button>
          </div>
          <p className="muted">{t("work.importHint")}</p>
          <label>
            {t("work.projectName")}
            <input
              name="name"
              value={form.name}
              maxLength={120}
              required
              onChange={(e) => {
                update("name", e.target.value);
                if (
                  !form.domain ||
                  form.domain ===
                    `${form.name
                      .toLowerCase()
                      .replace(/[^a-z0-9-]+/g, "-")
                      .replace(/^-|-$/g, "")}.devforge.test`
                )
                  update(
                    "domain",
                    `${e.target.value
                      .toLowerCase()
                      .replace(/[^a-z0-9-]+/g, "-")
                      .replace(/^-|-$/g, "")}.devforge.test`,
                  );
              }}
            />
          </label>
          <label>
            {t("work.domain")}
            <input
              name="domain"
              value={form.domain}
              required
              pattern="[a-z0-9][a-z0-9.\-]*\.devforge\.test"
              spellCheck={false}
              onChange={(e) => update("domain", e.target.value)}
            />
          </label>
          <div className="detail-grid">
            <RuntimeSelect
              kind="php"
              value={form.php}
              onChange={(v) => update("php", v)}
              list={runtimes}
            />
            <RuntimeSelect
              kind="node"
              value={form.node}
              onChange={(v) => update("node", v)}
              list={runtimes}
            />
          </div>
          <label className="toggle-row">
            <span>HTTPS</span>
            <input
              type="checkbox"
              checked={form.https}
              onChange={(e) => update("https", e.target.checked)}
            />
          </label>
          <div className="modal-actions">
            <button type="button" onClick={onClose}>
              {t("work.cancel")}
            </button>
            <button className="primary" type="submit">
              {busy ? t("work.working") : t("work.addProject")}
            </button>
          </div>
        </fieldset>
      </form>
    </Modal>
  );
}
function ConfirmDialog({
  errorText,
  busy,
  onClose,
  onConfirm,
}: {
  errorText: string;
  busy: boolean;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}) {
  const { t } = useTranslation();
  return (
    <Modal title={t("work.confirmRemove")} busy={busy} onClose={onClose}>
      <p>{t("work.removeHint")}</p>
      {errorText && (
        <p role="alert" className="modal-error">
          {errorText}
        </p>
      )}
      <div className="modal-actions">
        <button disabled={busy} onClick={onClose} data-initial-focus autoFocus>
          {t("work.cancel")}
        </button>
        <button
          className="danger"
          disabled={busy}
          onClick={() => void onConfirm()}
        >
          {t("work.remove")}
        </button>
      </div>
    </Modal>
  );
}
