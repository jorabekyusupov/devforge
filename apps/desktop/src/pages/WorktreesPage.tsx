import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Plus,
  GitBranch,
  FolderOpen,
  Archive,
  ExternalLink,
  AlertCircle,
} from "lucide-react";
import { WorkspaceModal } from "../components/WorkspaceModal";
import {
  useWorkspace,
  type WorkspaceBackend,
  type WorktreeInput,
  type WorktreePlan,
  type WorktreeCleanup,
} from "../workspace";
export function WorktreesPage({ backend }: { backend?: WorkspaceBackend }) {
  const { t, i18n } = useTranslation();
  const api = useWorkspace(backend);
  const [source, setSource] = useState(""),
    [selected, setSelected] = useState(""),
    [archived, setArchived] = useState(false);
  const [creating, setCreating] = useState(false),
    [cleanup, setCleanup] = useState<WorktreeCleanup | null>(null),
    [plan, setPlan] = useState<WorktreePlan | null>(null),
    [trusted, setTrusted] = useState(false),
    [consent, setConsent] = useState(false),
    [done, setDone] = useState("");
  const [input, setInput] = useState<WorktreeInput>({
    source_id: "",
    name: "",
    branch: "",
    base: "HEAD",
    new_branch: true,
    database: "empty",
    dependencies: true,
    copy_environment: false,
  });
  const projects = api.snapshot?.projects ?? [];
  const all = api.snapshot?.worktrees ?? [];
  const byId = new Map(projects.map((p) => [p.project.id, p]));
  const versions = new Map(
    (api.snapshot?.runtimes ?? []).map((r) => [r.id, r.version]),
  );
  const views = all.filter(
    (v) =>
      (archived || v.record.stage !== "removed") &&
      (!source || v.record.input.source_id === source),
  );
  const view = views.find((v) => v.record.id === selected) ?? views[0];
  const row = projects.find((p) => p.project.id === view?.record.project_id);
  useEffect(() => {
    if (plan) setTrusted(false);
  }, [plan]);
  function close() {
    setCreating(false);
    setCleanup(null);
    setPlan(null);
    setTrusted(false);
    setConsent(false);
    api.clearError();
  }
  function change(patch: Partial<WorktreeInput>) {
    setInput((v) => ({ ...v, ...patch }));
    setPlan(null);
  }
  function begin() {
    setInput({
      source_id: source || projects[0]?.project.id || "",
      name: "",
      branch: "",
      base: "HEAD",
      new_branch: true,
      database: "empty",
      dependencies: true,
      copy_environment: false,
    });
    setPlan(null);
    setDone("");
    setCreating(true);
    api.clearError();
  }
  function error(code: string) {
    return i18n.exists(`workspace.errors.${code}`)
      ? t(`workspace.errors.${code}`)
      : t("errors.unexpected");
  }
  const failure = api.error
    ? i18n.exists(`${api.error.scope}.errors.${api.error.code}`)
      ? t(`${api.error.scope}.errors.${api.error.code}`)
      : t("errors.unexpected")
    : "";
  const failureNotice = failure && (
    <div className="notice error" role="alert" tabIndex={-1}>
      <AlertCircle aria-hidden="true" />
      <p>{failure}</p>
    </div>
  );
  useEffect(() => {
    if (api.error)
      document.querySelector<HTMLElement>(".fork-dialog [role=alert]")?.focus();
  }, [api.error]);
  const status = (stage: string) =>
    t(`forks.stages.${stage}`, { defaultValue: t("forks.stages.failed") });
  async function review() {
    await api.run({ action: "worktree_plan", input }, (v) =>
      setPlan(v as unknown as WorktreePlan),
    );
  }
  async function create() {
    if (!plan) return;
    const ok = await api.run(
      { action: "worktree_create", input, review: plan.digest, trusted },
      (v) => setSelected(String(v.id)),
    );
    if (ok) {
      close();
      setDone(t("forks.created"));
    }
  }
  async function reviewCleanup() {
    if (!view) return;
    setDone("");
    setConsent(false);
    await api.run(
      { action: "worktree_cleanup_plan", id: view.record.id },
      (v) => setCleanup(v as unknown as WorktreeCleanup),
    );
  }
  async function remove() {
    if (!cleanup) return;
    const ok = await api.run({
      action: "worktree_cleanup",
      id: cleanup.workspace.record.id,
      review: cleanup.digest,
      consent,
    });
    if (ok) {
      close();
      setDone(t("forks.archived"));
      setArchived(true);
    }
  }
  return (
    <section
      className="workspace-content worktrees-page"
      aria-label={t("workspaces.title")}
    >
      <div className="workspace-toolbar">
        <label>
          {t("forks.source")}
          <select value={source} onChange={(e) => setSource(e.target.value)}>
            <option value="">{t("forks.allSources")}</option>
            {projects.map((p) => (
              <option key={p.project.id} value={p.project.id}>
                {p.project.name}
              </option>
            ))}
          </select>
        </label>
        <button
          className="primary"
          onClick={begin}
          disabled={api.busy || !projects.length}
        >
          <Plus aria-hidden="true" />
          {t("forks.new")}
        </button>
      </div>
      <p className="muted">{t("forks.committedOnly")}</p>
      <label className="check-label">
        <input
          type="checkbox"
          checked={archived}
          onChange={(e) => setArchived(e.target.checked)}
        />
        {t("forks.showArchived")}
      </label>
      {!creating && !cleanup && failureNotice}
      {api.busy && !creating && !cleanup && (
        <p role="status">{t("forks.working")}</p>
      )}
      {done && (
        <p className="notice" role="status">
          {done}
        </p>
      )}
      {!api.snapshot ? (
        <p role="status">{t("forks.loading")}</p>
      ) : !views.length ? (
        <div className="empty-state">
          <GitBranch aria-hidden="true" />
          <h2>{t("forks.empty")}</h2>
          <p>{t("forks.emptyHelp")}</p>
        </div>
      ) : (
        <>
          <div className="fork-list-heading" aria-hidden="true">
            <span>{t("forks.name")}</span>
            <span>{t("forks.branch")}</span>
            <span>{t("forks.source")}</span>
            <span>{t("forks.domain")}</span>
            <span>{t("forks.status")}</span>
          </div>
          <ul className="fork-list">
            {views.map((v) => (
              <li key={v.record.id}>
                <button
                  data-workspace={v.record.id}
                  className={`fork-row ${view?.record.id === v.record.id ? "selected" : ""}`}
                  aria-pressed={view?.record.id === v.record.id}
                  onClick={() => setSelected(v.record.id)}
                >
                  <strong>
                    {byId.get(v.record.project_id ?? "")?.project.name ??
                      v.record.input.name}
                  </strong>
                  <span className="mono" translate="no">
                    <GitBranch aria-hidden="true" />
                    {v.record.input.branch}
                  </span>
                  <span>
                    {projects.find(
                      (p) => p.project.id === v.record.input.source_id,
                    )?.project.name ?? t("forks.unregisteredSource")}
                  </span>
                  <span className="mono fork-domain" translate="no">
                    {byId.get(v.record.project_id ?? "")?.site?.domain ??
                      v.record.domain}
                  </span>
                  <span className="status-pill">{status(v.record.stage)}</span>
                </button>
              </li>
            ))}
          </ul>
          {view && (
            <article
              className="fork-detail"
              aria-labelledby="fork-detail-heading"
            >
              <h2 id="fork-detail-heading">
                {row?.project.name ?? view.record.input.name}
              </h2>
              <dl>
                <dt>{t("forks.branch")}</dt>
                <dd className="mono" translate="no">
                  {view.record.input.branch}
                </dd>
                <dt>{t("forks.source")}</dt>
                <dd>
                  {projects.find(
                    (p) => p.project.id === view.record.input.source_id,
                  )?.project.name ?? view.record.source_path}
                </dd>
                <dt>{t("forks.path")}</dt>
                <dd className="mono" translate="no">
                  {view.record.stage === "removed" || view.files_archived
                    ? view.retained_path
                    : view.path}
                </dd>
                <dt>{t("forks.domain")}</dt>
                <dd className="mono" translate="no">
                  {row?.site?.domain ?? view.record.domain}
                </dd>
                <dt>{t("forks.runtimes")}</dt>
                <dd className="mono" translate="no">
                  {versions.get(row?.project.php ?? "") ??
                    view.record.configuration.runtimes.php}{" "}
                  /{" "}
                  {versions.get(row?.project.node ?? "") ??
                    view.record.configuration.runtimes.node}
                </dd>
                <dt>{t("forks.database")}</dt>
                <dd>
                  {view.record.configuration.database?.engine === "mysql"
                    ? "MySQL"
                    : view.record.configuration.database?.engine ===
                        "postgresql"
                      ? "PostgreSQL"
                      : "SQLite"}{" "}
                  · {t(`forks.databaseModes.${view.record.input.database}`)}
                </dd>
                <dt>{t("forks.configuration")}</dt>
                <dd>{t("forks.pinned")}</dd>
              </dl>
              {view.record.last_error && (
                <p className="notice error" role="alert">
                  {error(view.record.last_error)}
                </p>
              )}
              <div className="workspace-actions">
                {row && (
                  <a
                    className="button primary"
                    href={`#projects?project=${encodeURIComponent(row.project.id)}`}
                  >
                    <ExternalLink aria-hidden="true" />
                    {t("forks.openProject")}
                  </a>
                )}
                <button
                  disabled={api.busy}
                  onClick={() =>
                    void api.run({
                      action: "worktree_open",
                      id: view.record.id,
                    })
                  }
                >
                  <FolderOpen aria-hidden="true" />
                  {t("forks.openFolder")}
                </button>
                {view.record.stage !== "removed" && (
                  <button
                    className="danger"
                    disabled={api.busy}
                    onClick={() => void reviewCleanup()}
                  >
                    <Archive aria-hidden="true" />
                    {t("forks.reviewCleanup")}
                  </button>
                )}
              </div>
              <p className="muted">{t("forks.cleanupPreserves")}</p>
            </article>
          )}
        </>
      )}
      {creating && (
        <WorkspaceModal
          title={t(plan ? "forks.reviewCreate" : "forks.new")}
          busy={api.busy}
          onClose={close}
          className="fork-dialog"
        >
          {failureNotice}
          {!plan ? (
            <form
              onSubmit={(e) => {
                e.preventDefault();
                void review();
              }}
            >
              <div className="form-grid">
                <label>
                  {t("forks.source")}
                  <select
                    required
                    name="source_id"
                    value={input.source_id}
                    onChange={(e) => change({ source_id: e.target.value })}
                  >
                    {projects.map((p) => (
                      <option key={p.project.id} value={p.project.id}>
                        {p.project.name}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  {t("forks.name")}
                  <input
                    required
                    maxLength={120}
                    name="name"
                    value={input.name}
                    onChange={(e) => change({ name: e.target.value })}
                    autoComplete="off"
                  />
                </label>
                <label>
                  {t("forks.branch")}
                  <input
                    required
                    maxLength={160}
                    name="branch"
                    value={input.branch}
                    onChange={(e) => change({ branch: e.target.value })}
                    autoComplete="off"
                    spellCheck={false}
                    placeholder="feature/search…"
                  />
                </label>
                <label>
                  {t("forks.base")}
                  <input
                    required
                    disabled={!input.new_branch}
                    name="base"
                    autoComplete="off"
                    value={input.base}
                    onChange={(e) => change({ base: e.target.value })}
                    spellCheck={false}
                  />
                </label>
                <label>
                  {t("forks.database")}
                  <select
                    name="database"
                    value={input.database}
                    onChange={(e) =>
                      change({ database: e.target.value as "empty" | "clone" })
                    }
                  >
                    <option value="empty">
                      {t("forks.databaseModes.empty")}
                    </option>
                    <option value="clone">
                      {t("forks.databaseModes.clone")}
                    </option>
                  </select>
                </label>
              </div>
              <label className="check-label">
                <input
                  type="checkbox"
                  name="new_branch"
                  checked={input.new_branch}
                  onChange={(e) => change({ new_branch: e.target.checked })}
                />
                {t("forks.newBranch")}
              </label>
              <label className="check-label">
                <input
                  type="checkbox"
                  name="dependencies"
                  checked={input.dependencies}
                  onChange={(e) => change({ dependencies: e.target.checked })}
                />
                {t("forks.dependencies")}
              </label>
              <label className="check-label">
                <input
                  type="checkbox"
                  name="copy_environment"
                  checked={input.copy_environment}
                  onChange={(e) =>
                    change({ copy_environment: e.target.checked })
                  }
                />
                {t("forks.copyEnvironment")}
              </label>
              <p className="muted">{t("forks.newHelp")}</p>
              <div className="modal-actions">
                <button type="button" disabled={api.busy} onClick={close}>
                  {t("work.cancel")}
                </button>
                <button className="primary" disabled={api.busy} type="submit">
                  {t("forks.review")}
                </button>
              </div>
            </form>
          ) : (
            <>
              <dl className="fork-review">
                <dt>{t("forks.name")}</dt>
                <dd>{input.name}</dd>
                <dt>{t("forks.branch")}</dt>
                <dd className="mono" translate="no">
                  {input.branch}
                </dd>
                <dt>{t("forks.commit")}</dt>
                <dd className="mono" translate="no">
                  {plan.commit}
                </dd>
                <dt>{t("forks.domain")}</dt>
                <dd className="mono" translate="no">
                  {plan.domain}
                </dd>
                <dt>{t("forks.database")}</dt>
                <dd>
                  {plan.sqlite ? "SQLite" : t("forks.managedDatabase")} ·{" "}
                  {t(`forks.databaseModes.${input.database}`)}
                </dd>
                <dt>{t("forks.environment")}</dt>
                <dd>
                  {input.copy_environment
                    ? plan.environment_names.join(", ") || t("forks.none")
                    : t("forks.environmentNotCopied")}
                </dd>
              </dl>
              {plan.source_dirty && (
                <p className="notice">{t("forks.dirtySource")}</p>
              )}
              {plan.blockers.map((code) => (
                <p key={code} className="notice error" role="alert">
                  {error(code)}
                </p>
              ))}
              <label className="check-label">
                <input
                  type="checkbox"
                  name="trusted"
                  checked={trusted}
                  onChange={(e) => setTrusted(e.target.checked)}
                  disabled={api.busy}
                />
                {t("forks.trust")}
              </label>
              <div className="modal-actions">
                <button disabled={api.busy} onClick={() => setPlan(null)}>
                  {t("forks.back")}
                </button>
                <button
                  className="primary"
                  disabled={api.busy || !trusted || !!plan.blockers.length}
                  onClick={() => void create()}
                >
                  {t("forks.create")}
                </button>
              </div>
            </>
          )}
          {api.busy && (
            <div className="operation-progress">
              <p role="status">{t("forks.working")}</p>
              <button onClick={() => void api.cancel()}>
                {t("forks.cancelOperation")}
              </button>
            </div>
          )}
        </WorkspaceModal>
      )}
      {cleanup && (
        <WorkspaceModal
          title={t("forks.reviewCleanup")}
          busy={api.busy}
          onClose={close}
          className="fork-dialog"
        >
          {failureNotice}
          <h3>{cleanup.workspace.record.input.name}</h3>
          <p>{t("forks.cleanupPreserves")}</p>
          <p className="muted">{t("forks.externalTools")}</p>
          <dl className="fork-review">
            <dt>{t("forks.branch")}</dt>
            <dd className="mono" translate="no">
              {cleanup.branch}
            </dd>
            <dt>{t("forks.commit")}</dt>
            <dd className="mono" translate="no">
              {cleanup.head || cleanup.workspace.record.commit}
            </dd>
            <dt>{t("forks.archivePath")}</dt>
            <dd className="mono" translate="no">
              {cleanup.workspace.retained_path}
            </dd>
            <dt>{t("forks.retainedDatabases")}</dt>
            <dd>
              {cleanup.databases.map((b) => b.database).join(", ") ||
                t("forks.localDataRetained")}
            </dd>
          </dl>
          {cleanup.blockers.map((code) => (
            <p key={code} className="notice error" role="alert">
              {error(code)}
            </p>
          ))}
          <label className="check-label">
            <input
              type="checkbox"
              name="consent"
              checked={consent}
              disabled={api.busy}
              onChange={(e) => setConsent(e.target.checked)}
            />
            {t("forks.cleanupConsent")}
          </label>
          <div className="modal-actions">
            <button disabled={api.busy} onClick={close}>
              {t("work.cancel")}
            </button>
            <button
              className="danger"
              disabled={api.busy || !consent || !!cleanup.blockers.length}
              onClick={() => void remove()}
            >
              {t("forks.archive")}
            </button>
          </div>
          {api.busy && <p role="status">{t("forks.working")}</p>}
        </WorkspaceModal>
      )}
    </section>
  );
}
