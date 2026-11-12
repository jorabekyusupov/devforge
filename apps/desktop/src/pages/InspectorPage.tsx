import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertCircle,
  Search,
  Radio,
  Trash2,
  Download,
  RefreshCw,
} from "lucide-react";
import {
  useWorkspace,
  type WorkspaceBackend,
  type InspectionView,
  type Command,
} from "../workspace";
export function InspectorPage({ backend }: { backend?: WorkspaceBackend }) {
  const { t, i18n } = useTranslation();
  const api = useWorkspace(backend);
  const projects = api.snapshot?.projects ?? [];
  const [project, setProject] = useState(
    () => new URLSearchParams(location.hash.split("?")[1]).get("project") ?? "",
  );
  const id = projects.some((p) => p.project.id === project)
    ? project
    : (projects[0]?.project.id ?? "");
  const [view, setView] = useState<InspectionView | null>(null),
    [kind, setKind] = useState("all"),
    [eventPage, setEventPage] = useState(0),
    [query, setQuery] = useState(""),
    [selected, setSelected] = useState(0),
    [failure, setFailure] = useState(false),
    [confirm, setConfirm] = useState<"install" | "clear" | null>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const installButton = useRef<HTMLButtonElement>(null),
    clearButton = useRef<HTMLButtonElement>(null);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let alive = true,
      timer: ReturnType<typeof setTimeout>;
    setView(null);
    setSelected(0);
    setFailure(false);
    async function poll() {
      try {
        if (!backend?.inspector) throw new Error();
        const value = await backend.inspector(id);
        if (alive) {
          setView(value);
          setFailure(false);
        }
      } catch {
        if (alive) setFailure(true);
      } finally {
        if (alive) timer = setTimeout(poll, 1200);
      }
    }
    if (id) void poll();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [id, backend, revision]);
  useEffect(() => {
    if (confirm) dialog.current?.showModal();
  }, [confirm]);
  const filtered = (view?.capture.events ?? []).filter(
    (e) =>
      (kind === "all" || e.kind === kind) &&
      `${e.name} ${JSON.stringify(e.data)}`
        .toLocaleLowerCase()
        .includes(query.toLocaleLowerCase()),
  );
  useEffect(() => setEventPage(0), [id, kind, query]);
  const pageIndex = Math.min(
    eventPage,
    Math.max(0, Math.ceil(filtered.length / 50) - 1),
  );
  const visible = filtered.slice(pageIndex * 50, pageIndex * 50 + 50);
  const row = visible.find((e) => e.sequence === selected) ?? visible[0];
  const profile = row ? view?.profiles[row.context] : null;
  const number = (value: number) =>
    new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 2 }).format(
      value,
    );
  async function execute(command: Command) {
    if (await api.run(command)) setRevision((n) => n + 1);
  }
  function close() {
    const action = confirm;
    dialog.current?.close();
    setConfirm(null);
    (action === "install" ? installButton : clearButton).current?.focus();
  }
  const error = api.error ? `${api.error.scope}.errors.${api.error.code}` : "";
  return (
    <section
      className="workspace-content inspector-content"
      aria-busy={api.busy}
    >
      {api.error && (
        <div role="alert" className="notice error">
          <AlertCircle aria-hidden="true" />
          <p>
            {i18n.exists(error) ? t(error) : t("inspector.operationFailed")}
          </p>
        </div>
      )}
      {!id ? (
        <div className="empty-projects">
          <h2>{t("inspector.noProjects")}</h2>
          <a href="#projects">{t("nav.projects")}</a>
        </div>
      ) : (
        <>
          <div className="inspector-toolbar">
            <label className="inspector-project">
              {t("inspector.project")}
              <select
                name="inspection-project"
                value={id}
                onChange={(e) => {
                  setProject(e.target.value);
                  location.hash = `inspector?project=${encodeURIComponent(e.target.value)}`;
                }}
              >
                {projects.map((p) => (
                  <option key={p.project.id} value={p.project.id}>
                    {p.project.name}
                  </option>
                ))}
              </select>
            </label>
            <div className="inspector-actions">
              {!view?.companion_installed && (
                <button
                  ref={installButton}
                  onClick={() => setConfirm("install")}
                  disabled={api.busy || !view}
                >
                  <Download aria-hidden="true" />
                  {t("inspector.install")}
                </button>
              )}
              <button
                className={view?.capture.enabled ? "capture-active" : ""}
                disabled={api.busy || !view?.companion_installed}
                onClick={() =>
                  void execute({
                    action: "inspector_capture",
                    id,
                    enabled: !view?.capture.enabled,
                  })
                }
              >
                <Radio aria-hidden="true" />
                {t(
                  view?.capture.enabled
                    ? "inspector.pause"
                    : "inspector.enable",
                )}
              </button>
              <button
                ref={clearButton}
                disabled={api.busy || !view?.capture.events.length}
                onClick={() => setConfirm("clear")}
              >
                <Trash2 aria-hidden="true" />
                {t("inspector.clear")}
              </button>
            </div>
          </div>
          <p className="muted inspector-hint">{t("inspector.localHint")}</p>
          {failure ? (
            <div className="notice error" role="alert">
              <AlertCircle aria-hidden="true" />
              <p>{t("inspector.loadFailed")}</p>
              <button onClick={() => setRevision((n) => n + 1)}>
                {t("common.retry")}
              </button>
            </div>
          ) : !view ? (
            <p role="status">{t("common.loading")}</p>
          ) : (
            <>
              <div className="inspector-filters">
                <label>
                  {t("inspector.kind")}
                  <select
                    value={kind}
                    onChange={(e) => setKind(e.target.value)}
                  >
                    <option value="all">{t("inspector.all")}</option>
                    {[
                      "request",
                      "query",
                      "job",
                      "log",
                      "dump",
                      "exception",
                    ].map((k) => (
                      <option key={k} value={k}>
                        {t(`inspector.kinds.${k}`)}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="inspector-search">
                  <span className="sr-only">{t("inspector.search")}</span>
                  <Search aria-hidden="true" />
                  <input
                    type="search"
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    placeholder={t("inspector.search")}
                    spellCheck={false}
                  />
                </label>
              </div>
              <div className="inspection-state" role="status">
                {t(
                  view.capture.enabled
                    ? "inspector.recording"
                    : "inspector.paused",
                )}{" "}
                · {t("inspector.retention")}
              </div>
              <div className="inspector-panels">
                <div className="inspector-table-wrap">
                  <table className="inspector-table">
                    <caption className="sr-only">
                      {t("inspector.events")}
                    </caption>
                    <thead>
                      <tr>
                        <th>{t("inspector.kind")}</th>
                        <th>{t("inspector.name")}</th>
                        <th>{t("inspector.duration")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {visible.map((e) => (
                        <tr
                          key={e.sequence}
                          className={
                            row?.sequence === e.sequence ? "selected" : ""
                          }
                        >
                          <td>{t(`inspector.kinds.${e.kind}`)}</td>
                          <td>
                            <button
                              className="event-select"
                              onClick={() => setSelected(e.sequence)}
                              aria-pressed={row?.sequence === e.sequence}
                            >
                              <span translate="no">{e.name}</span>
                              <small>
                                {new Intl.DateTimeFormat(i18n.language, {
                                  hour: "2-digit",
                                  minute: "2-digit",
                                  second: "2-digit",
                                }).format(new Date(e.received_ms))}
                              </small>
                            </button>
                          </td>
                          <td>{number(e.duration_ms)} ms</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  {filtered.length > 50 && (
                    <nav
                      className="inspector-pager"
                      aria-label={t("inspector.pages")}
                    >
                      <button
                        disabled={pageIndex === 0}
                        onClick={() => setEventPage(pageIndex - 1)}
                      >
                        {t("inspector.previous")}
                      </button>
                      <span>
                        {number(pageIndex * 50 + 1)}–
                        {number(Math.min(filtered.length, pageIndex * 50 + 50))}{" "}
                        / {number(filtered.length)}
                      </span>
                      <button
                        disabled={(pageIndex + 1) * 50 >= filtered.length}
                        onClick={() => setEventPage(pageIndex + 1)}
                      >
                        {t("inspector.next")}
                      </button>
                    </nav>
                  )}
                  {!filtered.length && (
                    <div className="inspector-empty">
                      <h2>{t("inspector.empty")}</h2>
                      <p>
                        {t(
                          !view.companion_installed
                            ? "inspector.setupHint"
                            : view.capture.enabled
                              ? "inspector.waitHint"
                              : "inspector.enableHint",
                        )}
                      </p>
                    </div>
                  )}
                </div>
                <section
                  className="inspector-detail"
                  aria-label={t("inspector.details")}
                  tabIndex={0}
                >
                  {row ? (
                    <>
                      <h2 translate="no">{row.name}</h2>
                      <p className="muted">
                        {t(`inspector.kinds.${row.kind}`)} ·{" "}
                        {number(row.duration_ms)} ms
                      </p>
                      {profile && (
                        <>
                          <dl className="inspector-metrics">
                            <div>
                              <dt>{t("inspector.contextTime")}</dt>
                              <dd>
                                {profile.duration_ms === null
                                  ? "—"
                                  : `${number(profile.duration_ms)} ms`}
                              </dd>
                            </div>
                            <div>
                              <dt>{t("inspector.queries")}</dt>
                              <dd>
                                {number(profile.captured_queries)} /{" "}
                                {number(profile.sql_ms)} ms
                              </dd>
                            </div>
                            <div>
                              <dt>{t("inspector.memory")}</dt>
                              <dd>
                                {profile.peak_memory_bytes === null
                                  ? "—"
                                  : `${number(profile.peak_memory_bytes / 1048576)} MiB`}
                              </dd>
                            </div>
                          </dl>
                          {profile.truncated && (
                            <p className="notice" role="status">
                              {t("inspector.truncated")}
                            </p>
                          )}
                          {!!profile.patterns.length && (
                            <table className="query-patterns">
                              <caption>{t("inspector.patterns")}</caption>
                              <thead>
                                <tr>
                                  <th>{t("inspector.name")}</th>
                                  <th>{t("inspector.calls")}</th>
                                  <th>{t("inspector.duration")}</th>
                                </tr>
                              </thead>
                              <tbody>
                                {profile.patterns.map((p) => (
                                  <tr key={p.sql}>
                                    <td>
                                      <code translate="no">{p.sql}</code>
                                      {(p.repeated || p.slow) && (
                                        <span className="inspection-warning">
                                          {t(
                                            p.repeated
                                              ? "inspector.repeated"
                                              : "inspector.slow",
                                          )}
                                        </span>
                                      )}
                                    </td>
                                    <td>{number(p.count)}</td>
                                    <td>{number(p.total_ms)} ms</td>
                                  </tr>
                                ))}
                              </tbody>
                            </table>
                          )}
                        </>
                      )}
                      <h3>{t("inspector.payload")}</h3>
                      <pre
                        className="inspection-payload"
                        translate="no"
                        tabIndex={0}
                      >
                        {JSON.stringify(row.data, null, 2)}
                      </pre>
                      <p className="small muted">{t("inspector.heuristic")}</p>
                    </>
                  ) : (
                    <p className="muted">{t("inspector.selectEvent")}</p>
                  )}
                </section>
              </div>
              <section className="work-panel inspector-capability">
                <div>
                  <h2>{t("inspector.phpCapability")}</h2>
                  <p className="muted">
                    {view.php_capability
                      ? t(
                          view.php_capability.loaded
                            ? "inspector.xdebugPresent"
                            : "inspector.xdebugMissing",
                        )
                      : t("inspector.probeHint")}
                  </p>
                  {view.php_capability?.version && (
                    <code>
                      {view.php_capability.version} · {view.php_capability.mode}
                    </code>
                  )}
                </div>
                <button
                  disabled={api.busy}
                  onClick={() =>
                    void execute({ action: "inspector_probe", id })
                  }
                >
                  <RefreshCw aria-hidden="true" />
                  {t("inspector.probe")}
                </button>
              </section>
            </>
          )}
        </>
      )}
      {confirm && (
        <dialog
          ref={dialog}
          className="inspector-dialog"
          aria-labelledby="inspector-dialog-title"
          onCancel={(e) => {
            e.preventDefault();
            if (!api.busy) close();
          }}
        >
          <h2 id="inspector-dialog-title">
            {t(
              confirm === "install"
                ? "inspector.installTitle"
                : "inspector.clearTitle",
            )}
          </h2>
          <p>
            {t(
              confirm === "install"
                ? "inspector.installHint"
                : "inspector.clearHint",
            )}
          </p>
          {api.error && (
            <p className="notice error" role="alert">
              {i18n.exists(error) ? t(error) : t("inspector.operationFailed")}
            </p>
          )}
          <div className="dialog-actions">
            <button disabled={api.busy} onClick={close} autoFocus>
              {t("work.cancel")}
            </button>
            <button
              disabled={api.busy}
              className="primary"
              onClick={async () => {
                const ok = await api.run({
                  action:
                    confirm === "install"
                      ? "inspector_install"
                      : "inspector_clear",
                  id,
                });
                if (ok) {
                  close();
                  setRevision((n) => n + 1);
                }
              }}
            >
              {t(
                api.busy
                  ? "work.working"
                  : confirm === "install"
                    ? "inspector.install"
                    : "inspector.clear",
              )}
            </button>
          </div>
        </dialog>
      )}
    </section>
  );
}
