import "./ProjectsWorkspace.css";
import "./ServicesWorkspace.css";
import { DatabaseSettingsDialog } from "../components/ServiceSettingsDialogs";
import { matchingDatabases, ProjectDetectionSummary } from "../components/ProjectDetectionSummary";
import { preferredRuntimeOptions } from "../runtime-branches";
import { RuntimeCatalog } from "../components/RuntimeCatalog";
import { runtimeVersionLabel } from "../runtime-branches";
import { FirstProjectChecklist } from "../components/FirstProjectChecklist";
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
  FolderPlus,
  FolderSearch,
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
  AlertCircle,
  X,
  Database as DatabaseIcon,
  CheckCircle2,
  Star,
  Globe,
  Wrench,
  SlidersHorizontal,
  ShieldCheck,
  RefreshCw,
  ScrollText,
  ChevronRight,
} from "lucide-react";
import {
  useWorkspace,
  type WorkspaceBackend,
  type ProjectDetection,
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
    [discoveryPath, setDiscoveryPath] = useState(""),
    [discoveryLimit, setDiscoveryLimit] = useState(50),
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
  const discovered = (api.snapshot?.discovery?.entries ?? []).filter(
    (entry) =>
      entry.state !== "ready" &&
      entry.state !== "detached" &&
      `${entry.name} ${entry.path}`
        .toLocaleLowerCase()
        .includes(query.toLocaleLowerCase()),
  );
  const [availableCatalog, setAvailableCatalog] = useState<Runtime[]>([]);
  useEffect(() => {
    if (mode !== "runtimes" || !backend?.catalog) return;
    let active = true;
    void backend
      .catalog()
      .then((rows) => {
        if (active)
          setAvailableCatalog(
            rows.map((row) => ({ ...row, installation_unknown: true })),
          );
      })
      .catch(() => {
        /* Keep the actionable workspace connection error visible. */
      });
    return () => {
      active = false;
    };
  }, [backend, mode]);
  const runtimes = api.snapshot?.runtimes ?? availableCatalog;
  useEffect(() => {
    const changed = () =>
      setSelected(
        new URLSearchParams(location.hash.split("?")[1]).get("project") ?? "",
      );
    window.addEventListener("hashchange", changed);
    return () => window.removeEventListener("hashchange", changed);
  }, []);
  const needsReconciliation =
    api.error?.code === "native_client_admission_unconfirmed";
  const needsConnection =
    !!api.error &&
    /^(native_bridge_|native_client_reauthentication_required|manager_auth_)/.test(
      api.error.code,
    );
  const errorKey = api.error
    ? `${api.error.scope}.errors.${api.error.code}`
    : "";
  const errorMessage = api.error
    ? needsReconciliation
      ? t("work.managerOperationUnconfirmed")
      : needsConnection
        ? t(
            api.error.code === "native_bridge_artifact_unavailable"
              ? "work.managerUnavailable"
              : "work.managerConnectionRequired",
          )
        : i18n.exists(errorKey)
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
            {!needsConnection && !needsReconciliation && (
              <a href="#runtimes">{t("work.checkRuntimes")}</a>
            )}
          </div>
          <button
            type="button"
            onClick={() =>
              void (needsReconciliation
                ? api.reconcile()
                : needsConnection
                  ? api.connect()
                  : api.refresh())
            }
            disabled={needsReconciliation ? api.connecting : api.busy}
          >
            {t(
              needsReconciliation
                ? "work.checkManagerOperation"
                : needsConnection
                  ? api.connecting
                    ? "work.connectingManager"
                    : "work.connectManager"
                  : "common.retry",
            )}
          </button>
          <button type="button" aria-label={t("work.dismiss")} onClick={api.clearError}>
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
            "update_runtime",
            "create_project",
            "prepare_project",
            "create_backup",
            "restore_backup",
            "install_redis_client",
          ].includes(api.pending?.action ?? "") && (
            <button type="button" onClick={() => void api.cancel()}>
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
            <div className="workspace-toolbar-actions">
              <button
                type="button"
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
                type="button"
                onClick={() => setAdding(true)}
                disabled={api.busy || !api.snapshot}
              >
                <FolderPlus aria-hidden="true" />
                {t("work.addProject")}
              </button>
            </div>
          </div>
          {api.snapshot?.discovery && (
            <section
              className={`discovery-projects ${discovered.length === 0 ? "discovery-projects-quiet" : ""}`}
              aria-labelledby="discovered-heading"
            >
              {discovered.length === 0 ? (
                <>
                  <div className="discovery-quiet-info">
                    <FolderSearch aria-hidden="true" />
                    <h2 id="discovered-heading">{t("discovery.discovered")}</h2>
                    <code
                      className="discovery-root-path"
                      title={api.snapshot.discovery.root ?? undefined}
                      translate="no"
                    >
                      {api.snapshot.discovery.root}
                    </code>
                  </div>
                  <a className="panel-header-link" href="#settings">
                    {t("discovery.manageFolder")}
                  </a>
                </>
              ) : (
                <>
                  <div className="discovery-list-heading">
                    <div className="discovery-heading-title">
                      <FolderSearch aria-hidden="true" />
                      <h2 id="discovered-heading">{t("discovery.discovered")}</h2>
                      <span className="discovery-count">{discovered.length}</span>
                      <code
                        className="discovery-root-path"
                        title={api.snapshot.discovery.root ?? undefined}
                        translate="no"
                      >
                        {api.snapshot.discovery.root}
                      </code>
                    </div>
                    <div className="discovery-header-links">
                      {api.snapshot.discovery.entries.some(
                        (e) => e.state === "needs_runtime",
                      ) && (
                        <a className="panel-header-link" href="#runtimes">
                          {t("discovery.installRuntimes")}
                        </a>
                      )}
                      <a className="panel-header-link" href="#settings">
                        {t("discovery.manageFolder")}
                      </a>
                    </div>
                  </div>
                  <ul>
                    {discovered.slice(0, discoveryLimit).map((entry) => (
                      <li
                        key={entry.identity}
                        data-discovery-identity={entry.identity}
                      >
                        <div>
                          <div className="discovery-entry-head">
                            <strong translate="no">{entry.name}</strong>
                            <span className="state">
                              {t(`discovery.states.${entry.state}`)}
                            </span>
                          </div>
                          <small className="discovery-entry-path" translate="no">
                            {entry.path}
                          </small>
                          {entry.problem && (
                            <small className="discovery-entry-problem">
                              {t(`discovery.problems.${entry.problem}`, {
                                defaultValue: t("discovery.failed"),
                              })}
                            </small>
                          )}
                          {entry.state === "needs_runtime" && entry.detection && (
                            <ProjectDetectionSummary
                              detection={entry.detection}
                              busy={api.busy}
                              install={async (packageId) => {
                                await api.run({ action: "install", package: packageId });
                              }}
                            />
                          )}
                        </div>
                        {entry.ignored ? (
                          <button
                            type="button"
                            disabled={api.busy}
                            onClick={() =>
                              void api.run({
                                action: "discovery_restore",
                                identity: entry.identity,
                              })
                            }
                          >
                            {t("discovery.restore")}
                          </button>
                        ) : (
                          !entry.project_id &&
                          entry.state !== "missing" &&
                          entry.state !== "detached" && (
                            <button
                              type="button"
                              disabled={api.busy}
                              onClick={() => {
                                setDiscoveryPath(entry.path);
                                api.clearError();
                                setAdding(true);
                              }}
                            >
                              {t("discovery.configure")}
                            </button>
                          )
                        )}
                      </li>
                    ))}
                  </ul>
                  {discovered.length > discoveryLimit && (
                    <div className="discovery-footer-actions">
                      <button
                        type="button"
                        onClick={() => setDiscoveryLimit((n) => n + 50)}
                      >
                        {t("discovery.showMore")}
                      </button>
                    </div>
                  )}
                </>
              )}
            </section>
          )}
          {api.snapshot && !api.busy && projects.length === 0 && (
            <div className="workspace-empty">
              <Folder aria-hidden="true" />
              <h2>{t("work.noProjects")}</h2>
              <p>{t("work.importHint")}</p>
              <div className="workspace-empty-actions">
                <button
                  type="button"
                  className="primary"
                  onClick={() => setAdding(true)}
                  disabled={api.busy}
                >
                  <FolderPlus aria-hidden="true" />
                  {t("work.addProject")}
                </button>
              </div>
            </div>
          )}
          {projects.length > 0 && (
            <div className="project-layout">
              <div className="project-rail">
                <div
                  className="project-filters"
                  role="group"
                  aria-label={t("setup.projectFilter")}
                >
                  {(["all", "recent", "favorites"] as const).map((key) => (
                    <button
                      type="button"
                      key={key}
                      aria-pressed={projectFilter === key}
                      onClick={() => setProjectFilter(key)}
                    >
                      {t(`setup.filters.${key}`)}
                    </button>
                  ))}
                </div>
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
                          <strong title={p.project.name} translate="no">
                            {p.project.name}
                          </strong>
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
                        <span title={p.site?.domain} translate="no">
                          {p.site?.domain}
                        </span>
                        <code title={p.project.path} translate="no">
                          {p.project.path}
                        </code>
                      </a>
                      <button
                        type="button"
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
                    <p className="muted project-list-empty">
                      {t("work.noMatches")}
                    </p>
                  )}
                </div>
              </div>
              {row && (
                <ProjectDetail
                  key={`${row.project.id}-${row.project.revision}`}
                  row={row}
                  snapshot={api.snapshot!}
                  logs={logs}
                  busy={api.busy}
                  run={api.run}
                  detect={backend?.detectProject}
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
        <RuntimeCatalog
          runtimes={runtimes}
          query={query}
          onQueryChange={setQuery}
          busy={api.busy}
          onRefresh={() => void api.run({ action: "refresh_runtimes" })}
        >
          {(visibleRuntimes) => (
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
                  {visibleRuntimes.map((group) => {
                    const p = group.latest;
                    const installed = group.installed.filter(
                      (runtime) => runtime.installed && !runtime.problem,
                    );
                    const memberIds = new Set(group.members.map((m) => m.id));
                    const usedByProjects = projects.filter(
                      (item) =>
                        memberIds.has(item.project.php) ||
                        memberIds.has(item.project.node) ||
                        (item.project.composer &&
                          memberIds.has(item.project.composer)),
                    ).length;
                    const remove = (runtime: Runtime) => (
                      <button
                        type="button"
                        key={runtime.id}
                        className="danger"
                        aria-label={t("work.removeRuntimeLabel", {
                          name: `${runtime.kind} ${runtime.version} ${runtime.variant}`,
                        })}
                        disabled={api.busy || p.installation_unknown}
                        onClick={() =>
                          setConfirm({
                            action: "remove_runtime",
                            package: runtime.id,
                          })
                        }
                      >
                        <Trash2 aria-hidden="true" />
                        {t("work.remove")}
                        {group.installed.length > 1 && (
                          <span>
                            {runtimeVersionLabel(runtime.version)} ·{" "}
                            {runtime.variant}
                          </span>
                        )}
                      </button>
                    );
                    return (
                      <tr key={group.id}>
                        <td>
                          <div className="runtime-name-cell">
                            <div className="runtime-name-head">
                              <strong>
                                {t(`work.kinds.${p.kind}`)} {group.branch}
                              </strong>
                              {p.kind !== "php" && (
                                <span className="op-pill-muted" translate="no">
                                  {p.variant}
                                </span>
                              )}
                            </div>
                            {usedByProjects > 0 && (
                              <span className="muted">
                                {t("work.projectUsers", {
                                  count: usedByProjects,
                                })}
                              </span>
                            )}
                          </div>
                        </td>
                        <td>
                          <div className="runtime-version-stack">
                            {installed.length > 0 && (
                              <span className="runtime-version-row">
                                <span>{t("runtimeCatalog.installedLabel")}:</span>
                                <code className="op-mono-chip" translate="no">
                                  {[
                                    ...new Set(
                                      installed.map((runtime) =>
                                        runtimeVersionLabel(runtime.version),
                                      ),
                                    ),
                                  ].join(", ")}
                                </code>
                              </span>
                            )}
                            <span className="runtime-version-row">
                              <span>{t("runtimeCatalog.latestLabel")}:</span>
                              <code className="op-mono-chip" translate="no">
                                {runtimeVersionLabel(p.version)}
                              </code>
                            </span>
                            {p.version !== runtimeVersionLabel(p.version) && (
                              <small translate="no">{p.version}</small>
                            )}
                            {p.replacement && !p.installed && (
                              <small>{t("work.runtimeSuperseded")}</small>
                            )}
                          </div>
                        </td>
                        <td>
                          <div className="runtime-status-stack">
                            {p.installation_unknown ? (
                              <span className="state">
                                {t("runtimeCatalog.connectionRequired")}
                              </span>
                            ) : (
                              <StateBadge
                                value={
                                  p.problem
                                    ? "damaged"
                                    : group.complete ||
                                        (installed.length > 0 &&
                                          group.updateAvailable)
                                      ? "installed"
                                      : installed.length > 0
                                        ? "incomplete"
                                        : "missing"
                                }
                              />
                            )}
                            {group.updateAvailable && (
                              <span className="runtime-update-note">
                                {t("runtimeCatalog.updateAvailable")}
                              </span>
                            )}
                          </div>
                        </td>
                        <td className="runtime-branch-actions">
                          <div className="runtime-actions-wrap">
                            {group.updateAvailable ? (
                              <button
                                type="button"
                                className="primary"
                                disabled={api.busy || p.installation_unknown}
                                onClick={() =>
                                  void api.run({
                                    action: "update_runtime",
                                    package: p.id,
                                  })
                                }
                              >
                                <Download aria-hidden="true" />
                                {t("runtimeCatalog.update")}
                              </button>
                            ) : !group.complete ? (
                              <button
                                type="button"
                                className={
                                  installed.length === 0 ? "primary" : undefined
                                }
                                disabled={api.busy || p.installation_unknown}
                                onClick={() =>
                                  void api.run({
                                    action: "install",
                                    package: p.replacement ?? p.id,
                                  })
                                }
                              >
                                <Download aria-hidden="true" />
                                {t(
                                  installed.length > 0
                                    ? "runtimeCatalog.completeInstallation"
                                    : p.replacement
                                      ? "work.installReplacement"
                                      : "work.install",
                                )}
                              </button>
                            ) : null}
                            {group.installed.length === 1 ? (
                              remove(group.installed[0])
                            ) : group.installed.length > 1 ? (
                              <details className="runtime-installed-builds">
                                <summary>
                                  <ChevronRight
                                    className="op-disclosure-chevron"
                                    aria-hidden="true"
                                  />
                                  <span>
                                    {t("runtimeCatalog.installedBuilds", {
                                      count: group.installed.length,
                                    })}
                                  </span>
                                </summary>
                                <div>{group.installed.map(remove)}</div>
                              </details>
                            ) : null}
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </RuntimeCatalog>
      )}
      {mode === "services" && (
        <>
          <DatabaseServices
            snapshot={api.snapshot}
            busy={api.busy}
            run={api.run}
            errorText={errorMessage}
            clearError={api.clearError}
          />
          <ProjectServices
            snapshot={api.snapshot}
            busy={api.busy}
            run={api.run}
            backend={backend}
            errorText={errorMessage}
            clearError={api.clearError}
          />
          <section className="work-panel services-logs-panel">
            <div className="section-toolbar">
              <h2>
                <ScrollText aria-hidden="true" />
                {t("work.logs")}
              </h2>
              <label>
                {t("work.stream")}
                <select
                  name="log-stream"
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
                  name="log-search"
                  autoComplete="off"
                  value={query}
                  placeholder={t("work.searchLogs")}
                  onChange={(e) => setQuery(e.target.value)}
                />
              </label>
            </div>
            <LogViewer logs={logs} />
          </section>
        </>
      )}
      {adding && (
        <ProjectDialog
          initialPath={discoveryPath}
          runtimes={runtimes}
          errorText={errorMessage}
          busy={api.busy}
          browse={api.browse}
          detect={backend?.detectProject}
          databases={api.snapshot?.databases ?? []}
          install={(packageId) => api.run({ action: "install", package: packageId })}
          onClose={() => setAdding(false)}
          onSubmit={async (value, instance) => {
            let id: unknown;
            if (await api.run(value, (result) => { id = result.project_id; })) {
              setAdding(false);
              if (instance && typeof id === "string") {
                await api.run({ action: "bind_database", id, instance });
              }
            }
          }}
        />
      )}
      {creating && (
        <CreateProjectDialog
          runtimes={runtimes}
          defaultParent={api.snapshot?.discovery?.root ?? ""}
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
function runtimeOptions(list: Runtime[], kind: string, selectedId?: string) {
  return preferredRuntimeOptions(list, selectedId).filter(
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
        {runtimeOptions(list, kind, value).map((p) => (
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
  logs,
  busy,
  run,
  detect,
  open,
  onForget,
  onTool,
}: {
  row: Row;
  snapshot: Snapshot;
  logs: Snapshot["logs"];
  busy: boolean;
  run: (c: Command) => Promise<boolean>;
  detect?: WorkspaceBackend["detectProject"];
  open: (
    id: string,
    t: "browser" | "editor" | "terminal" | "folder",
  ) => Promise<void>;
  onForget: () => void;
  onTool: (kind: "prepare" | "php" | "https") => void;
}) {
  const { t } = useTranslation();
  const p = row.project;
  const watchedSettings = snapshot.discovery?.entries.find(
    (entry) => entry.project_id === p.id && entry.state === "ready",
  )?.detection;
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
  const [detected, setDetected] = useState<ProjectDetection | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [detectError, setDetectError] = useState(false);
  async function redetect() {
    if (!detect) return;
    setDetecting(true);
    setDetectError(false);
    setDetected(null);
    try {
      setDetected(await detect({ id: p.id }));
    } catch {
      setDetectError(true);
    } finally {
      setDetecting(false);
    }
  }
  const dbSuggestions = matchingDatabases(detected?.database ?? null, snapshot.databases);

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
      <div className="project-hero">
        <div className="project-hero-top">
          <div className="detail-title">
            <Folder aria-hidden="true" />
            <h2 translate="no">{p.name}</h2>
            <StateBadge value={row.state} />
            {row.discovery_state && row.discovery_state !== "ready" && (
              <span className="state">
                {t(`discovery.states.${row.discovery_state}`)}
              </span>
            )}
          </div>
          <div className="project-url">
            <span className="project-url-chip">
              <Globe aria-hidden="true" />
              <code translate="no">
                {row.serving?.preferred_url ?? row.site?.domain}
              </code>
            </span>
            <button
              type="button"
              disabled={busy || row.state !== "running"}
              onClick={() => void open(p.id, "browser")}
            >
              <ExternalLink aria-hidden="true" />
              {t("work.openBrowser")}
            </button>
          </div>
        </div>
        <div className="action-bar project-actions">
          <div className="action-group lifecycle-actions">
            <button
              type="button"
              className={!active ? "primary" : undefined}
              disabled={
                busy ||
                active ||
                (!!row.discovery_state &&
                  !["ready", "detached"].includes(row.discovery_state))
              }
              onClick={() => void run({ action: "start", id: p.id })}
            >
              <Play aria-hidden="true" />
              {t("work.start")}
            </button>
            <button
              type="button"
              disabled={busy || !active}
              onClick={() => void run({ action: "stop", id: p.id })}
            >
              <Square aria-hidden="true" />
              {t("work.stop")}
            </button>
            <button
              type="button"
              disabled={busy || !active}
              onClick={() => void run({ action: "restart", id: p.id })}
            >
              <RotateCw aria-hidden="true" />
              {t("work.restart")}
            </button>
          </div>
          <div className="action-group quick-open-actions">
            <button
              type="button"
              disabled={busy}
              onClick={() => void open(p.id, "editor")}
            >
              <Code2 aria-hidden="true" />
              {t("work.openEditor")}
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void open(p.id, "terminal")}
            >
              <Terminal aria-hidden="true" />
              {t("work.openTerminal")}
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void open(p.id, "folder")}
            >
              <Folder aria-hidden="true" />
              {t("work.openFolder")}
            </button>
          </div>
        </div>
        <div className="action-bar setup-tools">
          <button
            type="button"
            disabled={busy || dirty}
            onClick={() => onTool("prepare")}
          >
            <Wrench aria-hidden="true" />
            {t("setup.prepare")}
          </button>
          <button
            type="button"
            id={`project-${p.id}-php-settings`}
            disabled={busy || dirty}
            onClick={() => onTool("php")}
          >
            <SlidersHorizontal aria-hidden="true" />
            {t("setup.phpSettings")}
          </button>
          <button
            type="button"
            disabled={busy || dirty}
            onClick={() => onTool("https")}
          >
            <ShieldCheck aria-hidden="true" />
            {t("setup.systemHttps")}
          </button>
        </div>
      </div>
      {row.serving && !row.serving.system_trusted && row.site?.https && (
        <p className="notice">{t("work.trustFallback")}</p>
      )}
      <FirstProjectChecklist
        row={row}
        snapshot={snapshot}
        busy={busy}
        dirty={dirty}
        onTool={onTool}
        openBrowser={() => void open(p.id, "browser")}
      />
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
              <div className="panel-header">
                <h3>{t("work.runtime")}</h3>
                {detect && (
                  <button
                    type="button"
                    className="panel-header-action"
                    disabled={detecting}
                    onClick={() => void redetect()}
                  >
                    <RefreshCw aria-hidden="true" />
                    {t("projectDetection.redetect")}
                  </button>
                )}
              </div>
              <p className="muted">{t("work.runtimeHint")}</p>
              {detecting && (
                <p role="status" className="muted">
                  {t("projectDetection.detecting")}
                </p>
              )}
              {detectError && (
                <p role="alert" className="modal-error">
                  {t("projectDetection.error")}
                </p>
              )}
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
                PHP {watchedSettings?.php_constraint ?? p.php_constraint} · Node{" "}
                {watchedSettings?.node_constraint ?? p.node_constraint}
              </p>
              {detected && (
                <>
                  <ProjectDetectionSummary
                    detection={detected}
                    busy={busy}
                    install={async (packageId) => {
                      if (await run({ action: "install", package: packageId }))
                        await redetect();
                    }}
                  />
                  <div className="detection-actions">
                    <button
                      type="button"
                      disabled={
                        !detected.php?.installed &&
                        !detected.node?.installed &&
                        (active || !!row.database || dbSuggestions.length !== 1)
                      }
                      onClick={() => {
                        setForm((f) => ({
                          ...f,
                          php: detected.php?.installed
                            ? detected.php.id
                            : f.php,
                          node: detected.node?.installed
                            ? detected.node.id
                            : f.node,
                        }));
                        if (
                          !active &&
                          !row.database &&
                          dbSuggestions.length === 1
                        ) {
                          setInstance(dbSuggestions[0].instance.id);
                        }
                      }}
                    >
                      {t("projectDetection.useSuggestions")}
                    </button>
                  </div>
                </>
              )}
              {watchedSettings && !detected && (
                <ProjectDetectionSummary
                  detection={watchedSettings}
                  metadataOnly
                />
              )}
            </section>
            <section className="work-panel">
              <div className="panel-header">
                <h3>{t("work.development")}</h3>
              </div>
              <div className="dev-toggle-list">
                <div className="dev-toggle-item">
                  <label className="toggle-row">
                    <span className="dev-toggle-label">{t("work.vite")}</span>
                    <input
                      type="checkbox"
                      role="switch"
                      checked={form.vite}
                      onChange={(e) => update("vite", e.target.checked)}
                    />
                  </label>
                </div>
                <div className="dev-toggle-item">
                  <label className="toggle-row">
                    <span className="dev-toggle-label">
                      {t("work.queue")}
                      {row.queue_state && <StateBadge value={row.queue_state} />}
                    </span>
                    <input
                      type="checkbox"
                      role="switch"
                      checked={form.queue}
                      onChange={(e) => update("queue", e.target.checked)}
                    />
                  </label>
                </div>
              </div>
            </section>
          </div>
          <details className="advanced-config">
            <summary>
              <span className="disclosure-title">
                <ChevronRight className="disclosure-chevron" aria-hidden="true" />
                {t("work.domainSettings")}
              </span>
              <span className="disclosure-preview">
                <code translate="no">{form.domain}</code>
                {form.https ? " · HTTPS" : ""}
              </span>
            </summary>
            <div className="advanced-config-body">
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
                  autoComplete="off"
                  pattern="[a-z0-9][a-z0-9.\-]*\.devforge\.test"
                  spellCheck={false}
                  onChange={(e) => update("domain", e.target.value)}
                />
              </label>
            </div>
          </details>
          <div className="save-row" data-dirty={dirty ? "true" : "false"}>
            <div className="save-row-copy">
              {dirty && (
                <span className="unsaved-badge">{t("work.unsaved")}</span>
              )}
              <p className="muted">{t("work.saveHint")}</p>
            </div>
            <button className={dirty ? "primary" : undefined} type="submit">
              {t("work.save")}
            </button>
          </div>
        </fieldset>
      </form>
      <section className="work-panel database-panel">
        <div className="panel-header">
          <div className="database-header-meta">
            <h3>
              <DatabaseIcon aria-hidden="true" />
              {t("work.database")}
            </h3>
            {row.database ? (
              <code className="database-name-chip" translate="no">
                {row.database.database}
              </code>
            ) : (
              <span className="muted">{t("work.noDatabase")}</span>
            )}
          </div>
          <a className="panel-header-link" href="#services">
            {t("work.manageDatabases")}
          </a>
        </div>
        <div className="database-controls">
          <label>
            {t("work.databaseService")}
            <select
              name="database-service"
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
            type="button"
            disabled={busy || active || !instance || !!row.database}
            onClick={() =>
              void run({ action: "bind_database", id: p.id, instance })
            }
          >
            {t("work.bindDatabase")}
          </button>
          <button
            type="button"
            disabled={busy || !row.database}
            onClick={() => void run({ action: "test_database", id: p.id })}
          >
            {t("work.testConnection")}
          </button>
        </div>
      </section>
      <section className="work-panel project-logs-panel">
        <div className="panel-header">
          <h3>
            <ScrollText aria-hidden="true" />
            {t("work.logs")}
          </h3>
          <a
            className="panel-header-link"
            href={`#services?project=${encodeURIComponent(p.id)}`}
          >
            {t("work.allLogs")}
          </a>
        </div>
        <LogViewer logs={logs} />
      </section>
      <details className="project-meta">
        <summary>
          <span className="disclosure-title">
            <ChevronRight className="disclosure-chevron" aria-hidden="true" />
            {t("work.projectDetails")}
          </span>
        </summary>
        <div className="project-meta-body">
          <dl>
            <dt>{t("work.path")}</dt>
            <dd>
              <code translate="no">{p.path}</code>
            </dd>
            <dt>{t("work.domain")}</dt>
            <dd translate="no">{row.site?.domain}</dd>
          </dl>
          <div className="project-danger-row">
            <button
              type="button"
              className="danger"
              disabled={busy}
              onClick={onForget}
            >
              <Trash2 aria-hidden="true" />
              {t("work.forget")}
            </button>
            <p className="muted">{t("work.dataPreserved")}</p>
          </div>
        </div>
      </details>
      {leaveTo && (
        <Modal
          title={t("work.unsaved")}
          busy={false}
          onClose={() => setLeaveTo(null)}
        >
          <p>{t("work.unsavedHint")}</p>
          <div className="modal-actions">
            <button
              type="button"
              onClick={() => setLeaveTo(null)}
              data-initial-focus
            >
              {t("work.keepEditing")}
            </button>
            <button
              type="button"
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
  errorText,
  clearError,
}: {
  snapshot: Snapshot | null;
  busy: boolean;
  run: (c: Command) => Promise<boolean>;
  errorText: string;
  clearError: () => void;
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
  const [customPort, setCustomPort] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const value = selected || list[0]?.id || "";
  const trimmedPort = customPort.trim();
  const parsedPort = trimmedPort ? Number.parseInt(trimmedPort, 10) : undefined;
  const portInvalid =
    trimmedPort !== "" &&
    (!Number.isInteger(parsedPort) ||
      (parsedPort ?? 0) < 1024 ||
      (parsedPort ?? 0) > 65535);
  const editing = editingId
    ? snapshot?.databases.find((d) => d.instance.id === editingId)
    : undefined;
  return (
    <section className="work-panel">
      <div className="section-toolbar">
        <h2>
          <DatabaseIcon aria-hidden="true" />
          {t("work.databaseServices")}
          {snapshot && snapshot.databases.length > 0 && (
            <span className="service-count-badge">
              {snapshot.databases.length}
            </span>
          )}
        </h2>
        <div className="service-create-controls">
          <label>
            {t("work.version")}
            <select
              name="create-database-version"
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
          <label className="service-port-field">
            {t("services.port")}
            <input
              name="create-database-port"
              type="number"
              inputMode="numeric"
              min={1024}
              max={65535}
              placeholder={t("services.portAuto")}
              disabled={busy}
              value={customPort}
              onChange={(e) => setCustomPort(e.target.value)}
            />
          </label>
          <button
            type="button"
            className="primary"
            disabled={
              busy ||
              portInvalid ||
              !list.find((p) => p.id === value)?.installed
            }
            onClick={() =>
              void run({
                action: "create_database",
                package: value,
                ...(parsedPort ? { port: parsedPort } : {}),
              }).then((ok) => {
                if (ok) setCustomPort("");
              })
            }
          >
            <Plus aria-hidden="true" />
            {t("work.createDatabase")}
          </button>
        </div>
      </div>
      <p className="muted">{t("work.databaseHint")}</p>
      {!snapshot?.databases.length ? (
        <p className="service-empty-box">{t("work.noServices")}</p>
      ) : (
        <div className="service-instance-list">
          {snapshot.databases.map((d) => {
            const version = snapshot.runtimes.find(
              (r) => r.id === d.instance.package,
            )?.version;
            const s = d.instance.settings;
            const params: string[] = [];
            if (s?.memory_mb) params.push(`${s.memory_mb} MB`);
            if (s?.max_connections) params.push(`max ${s.max_connections}`);
            if (s?.work_mem_mb) params.push(`work_mem ${s.work_mem_mb} MB`);
            if (s?.max_allowed_packet_mb)
              params.push(`packet ${s.max_allowed_packet_mb} MB`);
            if (s?.timezone) params.push(s.timezone);
            if (s?.log_statement && s.log_statement !== "none")
              params.push(`log: ${s.log_statement}`);
            if (s?.sql_mode && s.sql_mode !== "strict") params.push(s.sql_mode);
            return (
              <div className="service-row" key={d.instance.id}>
                <div className="service-row-identity">
                  <div className="service-row-title">
                    <DatabaseIcon aria-hidden="true" />
                    <strong>
                      {d.instance.engine === "mysql" ? "MySQL" : "PostgreSQL"}
                      {version ? ` · ${version}` : ""}
                    </strong>
                    <code className="service-address-chip" translate="no">
                      127.0.0.1:{d.instance.port}
                    </code>
                    <StateBadge value={d.state} />
                  </div>
                  <div className="service-meta-chips">
                    <span>{t("work.projectUsers", { count: d.users })}</span>
                    {params.map((chip) => (
                      <span
                        key={chip}
                        className="service-param-chip"
                        translate="no"
                      >
                        {chip}
                      </span>
                    ))}
                  </div>
                </div>
                <div className="service-row-actions">
                  <button
                    type="button"
                    disabled={busy || !d.instance.ready}
                    onClick={() => {
                      clearError();
                      setEditingId(d.instance.id);
                    }}
                  >
                    <SlidersHorizontal aria-hidden="true" />
                    {t("services.settings")}
                  </button>
                  <button
                    type="button"
                    disabled={busy || d.users > 0 || !d.instance.ready}
                    onClick={() =>
                      void run({
                        action:
                          d.state === "stopped"
                            ? "start_database"
                            : "stop_database",
                        instance: d.instance.id,
                      })
                    }
                  >
                    {d.state === "stopped" ? (
                      <Play aria-hidden="true" />
                    ) : (
                      <Square aria-hidden="true" />
                    )}
                    {t(d.state === "stopped" ? "work.start" : "work.stop")}
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
      {editing && (
        <DatabaseSettingsDialog
          key={`${editing.instance.id}-${editing.instance.port}`}
          database={editing}
          version={
            snapshot?.runtimes.find((r) => r.id === editing.instance.package)
              ?.version
          }
          busy={busy}
          errorText={errorText}
          onClose={() => setEditingId(null)}
          onSave={run}
        />
      )}
    </section>
  );
}
function LogViewer({ logs }: { logs: Snapshot["logs"] }) {
  const { t } = useTranslation();
  return (
    <div className="log-viewer" tabIndex={0} aria-label={t("work.logs")}>
      {logs.length ? (
        logs.map((log, i) => (
          <div
            key={`${log.process}-${i}`}
            className="log-line"
            data-stream={log.stream}
          >
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
  initialPath = "",
  runtimes,
  errorText,
  busy,
  browse,
  detect,
  databases,
  install,
  onClose,
  onSubmit,
}: {
  runtimes: Runtime[];
  errorText: string;
  busy: boolean;
  browse: () => Promise<string | null>;
  detect?: WorkspaceBackend["detectProject"];
  databases: Snapshot["databases"];
  install: (id: string) => Promise<boolean>;
  onClose: () => void;
  initialPath?: string;
  onSubmit: (v: Extract<Command, { action: "import" }>, instance: string) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [path, setPath] = useState(initialPath);
  const [form, setForm] = useState<ProjectInput>({
    name: "",
    php: runtimeOptions(runtimes, "php")[0]?.id ?? "",
    node: runtimeOptions(runtimes, "node")[0]?.id ?? "",
    domain: "",
    https: true,
    vite: false,
    queue: false,
  });
  const [scan, setScan] = useState<{ path: string; value?: ProjectDetection } | null>(null);
  const [scanAgain, setScanAgain] = useState(0);
  const [databaseChoice, setDatabaseChoice] = useState<{ path: string; instance: string } | null>(null);
  const requestedPath = path.trim();
  const detected = scan?.path === requestedPath ? scan.value : undefined;
  const detecting = Boolean(detect && requestedPath && scan?.path !== requestedPath);
  const dbSuggestions = matchingDatabases(detected?.database ?? null, databases);
  const database = databaseChoice?.path === requestedPath
    ? databaseChoice.instance
    : dbSuggestions.length === 1 && detected?.database?.source !== "composer.json"
      ? dbSuggestions[0].instance.id
      : "";
  const missingRuntime = detected && (
    !detected.php || !detected.node ||
    (detected.php.id === form.php && !detected.php.installed) ||
    (detected.node.id === form.node && !detected.node.installed)
  );

  useEffect(() => {
    if (!detect || !requestedPath) return;
    let current = true;
    const timer = window.setTimeout(() => {
      void detect({ path: requestedPath }).then((value) => {
        if (!current) return;
        setScan({ path: requestedPath, value });
        setForm((f) => ({
          ...f,
          php: value.php?.id ?? f.php,
          node: value.node?.id ?? f.node,
        }));
      }).catch(() => {
        if (current) setScan({ path: requestedPath });
      });
    }, 350);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [detect, requestedPath, scanAgain]);

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
          if (!detecting && !missingRuntime) {
            void onSubmit({ action: "import", path: requestedPath, ...form }, database);
          }
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
          {detecting && <p role="status">{t("projectDetection.detecting")}</p>}
          {requestedPath && scan?.path === requestedPath && !detected && (
            <p role="alert">{t("projectDetection.error")}</p>
          )}
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
          {detected && (
            <ProjectDetectionSummary detection={detected} busy={busy} install={async (id) => {
              if (await install(id)) {
                setScan(null);
                setScanAgain((value) => value + 1);
              }
            }} />
          )}
          {dbSuggestions.length > 0 && (
            <label>
              {t("work.databaseService")}
              <select name="import-database" value={database} onChange={(e) => setDatabaseChoice({ path: requestedPath, instance: e.target.value })}>
                <option value="">{t("projectDetection.skipDatabase")}</option>
                {dbSuggestions.map(({ instance }) => (
                  <option key={instance.id} value={instance.id}>{instance.engine === "mysql" ? "MySQL" : "PostgreSQL"} · {instance.port}</option>
                ))}
              </select>
            </label>
          )}
          {detected?.database && dbSuggestions.length === 0 && ["mysql", "postgresql"].includes(detected.database.engine) && (
            <p className="muted">{t("projectDetection.noDatabaseService")}</p>
          )}
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
            <button className="primary" type="submit" disabled={detecting || !!missingRuntime}>
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
