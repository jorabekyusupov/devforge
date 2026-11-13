import { useEffect, useState, useRef } from "react";
import { useTranslation } from "react-i18next";
import {
  Play,
  Square,
  ArrowRight,
  ArrowDown,
  ArrowUp,
  Plus,
  Trash2,
  Download,
  Settings2,
} from "lucide-react";
import { WorkspaceModal } from "../components/WorkspaceModal";
import {
  useWorkspace,
  type WorkspaceBackend,
  type DebugView,
  type DebugSettings,
  type DebugMode,
  type DebugVariable,
  type FunctionProfile,
} from "../workspace";
function Variables({ values }: { values: DebugVariable[] }) {
  return (
    <ul className="debug-variables">
      {values.map((v, i) => (
        <li key={v.name + String(i)}>
          <span className="mono" translate="no">
            {v.name}
          </span>
          <span className="muted" translate="no">
            {v.kind}
          </span>
          <code translate="no">{v.value || "—"}</code>
          {!!v.children.length && (
            <details>
              <summary>{v.children.length}</summary>
              <Variables values={v.children} />
            </details>
          )}
        </li>
      ))}
    </ul>
  );
}
export function DebuggerPage({ backend }: { backend?: WorkspaceBackend }) {
  const { t, i18n } = useTranslation();
  const api = useWorkspace(backend);
  const [chosen, setChosen] = useState(""),
    [view, setView] = useState<DebugView | null>(null),
    [readError, setReadError] = useState<string | null>(null),
    [tab, setTab] = useState<"debug" | "profiles">("debug"),
    [request, setRequest] = useState("/"),
    [file, setFile] = useState(""),
    [line, setLine] = useState(1),
    [code, setCode] = useState<{ file: string; code: string } | null>(null),
    [profile, setProfile] = useState<FunctionProfile | null>(null),
    [selectedProfile, setSelectedProfile] = useState(""),
    [query, setQuery] = useState(""),
    [configure, setConfigure] = useState(false),
    [draft, setDraft] = useState<DebugSettings>({
      mode: "off",
      runtime: null,
      breakpoints: [],
    }),
    [consent, setConsent] = useState(false),
    [installing, setInstalling] = useState(false),
    [remove, setRemove] = useState<string | null>(null);
  const rows = api.snapshot?.projects ?? [];
  const row = rows.find((r) => r.project.id === chosen) ?? rows[0];
  const id = row?.project.id ?? "";
  const currentId = useRef(id);
  currentId.current = id;
  const profileRead = useRef(0);
  const busy = api.busy;
  const paused = view?.session?.state === "paused";
  const running = row?.state === "running";
  const hasSession = !!row && row.state !== "stopped";
  const frame =
    view?.session?.frames.find((f) => f.depth === view.session?.depth) ??
    view?.session?.frames[0];
  const message = (code: string) =>
    i18n.exists(`debugger.errors.${code}`)
      ? t(`debugger.errors.${code}`)
      : t("errors.unexpected");
  async function refresh() {
    if (!id || !backend?.debugView) return;
    try {
      const next = await backend.debugView(id);
      if (currentId.current !== id) return;
      setView(next);
      setReadError(null);
    } catch {
      setReadError(t("debugger.errors.unavailable"));
    }
  }
  useEffect(() => {
    setView(null);
    setCode(null);
    setProfile(null);
    setSelectedProfile("");
    profileRead.current++;
  }, [id]);
  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;

    async function poll() {
      if (!busy && id && backend?.debugView) {
        try {
          const next = await backend.debugView(id);
          if (alive) {
            setView(next);
            setReadError(null);
          }
        } catch {
          if (alive) setReadError(t("debugger.errors.unavailable"));
        }
      }
      if (alive) timer = setTimeout(poll, 500);
    }
    void poll();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [id, backend, busy, i18n.language]);
  useEffect(() => {
    let alive = true;
    const path = frame?.file;
    if (path && id && backend?.debugSource) {
      void backend
        .debugSource(id, path)
        .then((v) => {
          if (alive) setCode(v);
        })
        .catch(() => {
          if (alive) setCode(null);
        });
    } else setCode(null);
    return () => {
      alive = false;
    };
  }, [id, frame?.file, backend]);
  async function execute(command: Parameters<typeof api.run>[0]) {
    const result = await api.run(command);
    if (result) await refresh();
    return result;
  }
  async function control(
    command: "run" | "step_into" | "step_over" | "step_out" | "stop" | "detach",
  ) {
    await execute({ action: "debug_control", id, control: { command } });
  }
  async function breakpoint() {
    if (!view) return;
    if (
      await execute({
        action: "debug_breakpoints",
        id,
        revision: row!.project.revision,
        breakpoints: [...view.settings.breakpoints, { file, line }],
      })
    ) {
      setFile("");
    }
  }
  async function loadProfile(key: string) {
    const ticket = ++profileRead.current;
    setSelectedProfile(key);
    setProfile(null);
    try {
      const result = await backend?.debugProfile?.(id, key);
      if (result && currentId.current === id && profileRead.current === ticket)
        setProfile(result);
    } catch {
      if (currentId.current !== id || profileRead.current !== ticket) return;
      setReadError(t("debugger.errors.profile_invalid"));
    }
  }
  async function saveProfile() {
    try {
      const path = await backend?.chooseDebugProfile?.();
      if (path)
        await execute({
          action: "debug_profile_export",
          id,
          profile: selectedProfile,
          path,
        });
    } catch {
      setReadError(t("debugger.errors.unavailable"));
    }
  }
  const error = api.error
    ? i18n.exists(`${api.error.scope}.errors.${api.error.code}`)
      ? t(`${api.error.scope}.errors.${api.error.code}`)
      : message(api.error.code)
    : readError;
  const errorNotice = error && (
    <p className="notice error" role="alert">
      {error}
    </p>
  );
  function settings() {
    if (!view) return;
    setDraft({
      ...view.settings,
      runtime:
        view.settings.runtime ??
        view.runtimes.find((r) => r.installed)?.id ??
        null,
    });
    setConsent(false);
    setConfigure(true);
    api.clearError();
  }
  const values =
    profile?.functions.filter((f) =>
      (f.name + f.file)
        .toLocaleLowerCase(i18n.language)
        .includes(query.toLocaleLowerCase(i18n.language)),
    ) ?? [];
  const numberFormat = new Intl.NumberFormat(i18n.language, {
    maximumFractionDigits: 2,
  });
  const dateFormat = new Intl.DateTimeFormat(i18n.language, {
    dateStyle: "medium",
    timeStyle: "short",
  });
  const number = (n: number) => numberFormat.format(n);
  const displayLines =
    code?.code
      .split(/\r?\n/)
      .map((text, index) => ({ text, line: index + 1 }))
      .filter((v) =>
        frame?.line
          ? v.line >= Math.max(1, frame.line - 12) && v.line <= frame.line + 16
          : v.line <= 40,
      ) ?? [];
  return (
    <section
      className="workspace-content debugger-content"
      aria-label={t("debugger.title")}
    >
      <div className="workspace-toolbar">
        <label>
          {t("debugger.project")}
          <select
            name="debug-project"
            disabled={busy}
            value={id}
            onChange={(e) => setChosen(e.target.value)}
          >
            {rows.map((r) => (
              <option key={r.project.id} value={r.project.id}>
                {r.project.name}
              </option>
            ))}
          </select>
        </label>
        <button disabled={!view || busy} onClick={settings}>
          <Settings2 aria-hidden="true" />
          {t("debugger.configure")}
        </button>
      </div>
      {!configure && !installing && !remove && errorNotice}
      {!rows.length ? (
        <div className="empty-state">
          <h2>{t("debugger.empty")}</h2>
          <p>{t("debugger.emptyHelp")}</p>
        </div>
      ) : !view ? (
        <p role="status">{t("debugger.loading")}</p>
      ) : (
        <>
          <div className="debug-runtime-row">
            <strong>{t(`debugger.modes.${view.settings.mode}`)}</strong>
            <span>
              {view.runtimes.some((r) => r.installed)
                ? t("debugger.sdkReady")
                : t("debugger.sdkMissing")}
            </span>
            <button
              disabled={busy || row?.state !== "stopped"}
              onClick={() => {
                setConsent(false);
                setInstalling(true);
                api.clearError();
              }}
            >
              <Download aria-hidden="true" />
              {t("debugger.install")}
            </button>
          </div>
          <div className="activity-tabs" aria-label={t("debugger.views")}>
            <button
              aria-pressed={tab === "debug"}
              onClick={() => setTab("debug")}
            >
              {t("debugger.tabDebug")}
            </button>
            <button
              aria-pressed={tab === "profiles"}
              onClick={() => setTab("profiles")}
            >
              {t("debugger.tabProfiles")}
            </button>
          </div>
          <div className="debug-controls">
            <span className="status-pill" role="status">
              {view.session
                ? t(`debugger.states.${view.session.state}`, {
                    defaultValue: view.session.state,
                  })
                : running && view.settings.mode === "profile"
                  ? t("debugger.states.profiling")
                  : t("debugger.states.stopped")}
            </span>
            <button
              className="primary"
              disabled={!paused || busy}
              onClick={() => void control("run")}
            >
              <Play aria-hidden="true" />
              {t("debugger.continue")}
            </button>
            <button
              disabled={!paused || busy}
              onClick={() => void control("step_over")}
            >
              <ArrowRight aria-hidden="true" />
              {t("debugger.stepOver")}
            </button>
            <button
              disabled={!paused || busy}
              onClick={() => void control("step_into")}
            >
              <ArrowDown aria-hidden="true" />
              {t("debugger.stepInto")}
            </button>
            <button
              disabled={!paused || busy}
              onClick={() => void control("step_out")}
            >
              <ArrowUp aria-hidden="true" />
              {t("debugger.stepOut")}
            </button>
            <button
              disabled={!paused || busy}
              onClick={() => void control("stop")}
            >
              <Square aria-hidden="true" />
              {t("debugger.stopRequest")}
            </button>
          </div>
          <div className="debug-request">
            <label>
              {t("debugger.requestPath")}
              <input
                name="debug-request-path"
                value={request}
                onChange={(e) => setRequest(e.target.value)}
                spellCheck={false}
                autoComplete="off"
              />
            </label>
            <button
              disabled={
                busy ||
                !running ||
                view.settings.mode === "off" ||
                view.request?.state === "running"
              }
              onClick={() =>
                void execute({ action: "debug_request", id, path: request })
              }
            >
              {t("debugger.runRequest")}
            </button>
            <button
              disabled={busy}
              onClick={() =>
                void execute({ action: hasSession ? "stop" : "start", id })
              }
            >
              {t(hasSession ? "debugger.stopProject" : "debugger.startProject")}
            </button>
          </div>
          {view.request && (
            <p className="muted" role="status">
              {t(`debugger.requests.${view.request.state}`)}
              {view.request.status !== null
                ? ` · HTTP ${view.request.status}`
                : ""}
            </p>
          )}
          {view.session?.error && (
            <p className="notice error" role="alert">
              {message(view.session.error)}
            </p>
          )}
          {tab === "debug" ? (
            <>
              <div className="debug-main-grid">
                <article className="debug-panel">
                  <h2>{t("debugger.breakpoints")}</h2>
                  <ul className="debug-breakpoints">
                    {view.settings.breakpoints.map((b, index) => (
                      <li key={b.file + ":" + b.line}>
                        <code translate="no">
                          {b.file}:{b.line}
                        </code>
                        <button
                          disabled={busy || view.session?.state === "running"}
                          aria-label={t("debugger.removeBreakpoint")}
                          onClick={() =>
                            void execute({
                              action: "debug_breakpoints",
                              id,
                              revision: row!.project.revision,
                              breakpoints: view.settings.breakpoints.filter(
                                (_, i) => i !== index,
                              ),
                            })
                          }
                        >
                          <Trash2 aria-hidden="true" />
                        </button>
                      </li>
                    ))}
                  </ul>
                  <form
                    onSubmit={(e) => {
                      e.preventDefault();
                      void breakpoint();
                    }}
                  >
                    <label>
                      {t("debugger.file")}
                      <input
                        name="breakpoint-file"
                        required
                        maxLength={512}
                        value={file}
                        onChange={(e) => setFile(e.target.value)}
                        autoComplete="off"
                        spellCheck={false}
                        placeholder="app/Http/Controllers/Controller.php…"
                      />
                    </label>
                    <label>
                      {t("debugger.line")}
                      <input
                        name="breakpoint-line"
                        autoComplete="off"
                        required
                        type="number"
                        min={1}
                        max={1000000}
                        value={line}
                        onChange={(e) => setLine(e.target.valueAsNumber)}
                      />
                    </label>
                    <button
                      disabled={busy || view.session?.state === "running"}
                      type="submit"
                    >
                      <Plus aria-hidden="true" />
                      {t("debugger.add")}
                    </button>
                  </form>
                </article>
                <article className="debug-panel debug-source">
                  <h2 translate={code ? "no" : undefined}>
                    {code?.file ?? t("debugger.source")}
                  </h2>
                  {code ? (
                    <div
                      className="debug-code"
                      tabIndex={0}
                      role="region"
                      aria-label={t("debugger.source")}
                    >
                      <ol>
                        {displayLines.map((v) => (
                          <li
                            value={v.line}
                            key={v.line}
                            className={v.line === frame?.line ? "current" : ""}
                            aria-current={
                              v.line === frame?.line ? "step" : undefined
                            }
                          >
                            <code translate="no">{v.text || " "}</code>
                          </li>
                        ))}
                      </ol>
                    </div>
                  ) : (
                    <p className="muted">{t("debugger.sourceHelp")}</p>
                  )}
                </article>
              </div>
              <div className="debug-bottom-grid">
                <article className="debug-panel">
                  <h2>{t("debugger.stack")}</h2>
                  <ul className="debug-stack">
                    {view.session?.frames.map((f) => (
                      <li key={f.depth}>
                        <button
                          disabled={!paused || busy}
                          aria-pressed={f.depth === view.session?.depth}
                          onClick={() =>
                            void execute({
                              action: "debug_control",
                              id,
                              control: { command: "frame", depth: f.depth },
                            })
                          }
                        >
                          <code translate="no">{f.function}</code>
                          <span translate="no">
                            {f.file ?? t("debugger.internal")}:{f.line}
                          </span>
                        </button>
                      </li>
                    ))}
                  </ul>
                  {!view.session?.frames.length && (
                    <p className="muted">{t("debugger.stackEmpty")}</p>
                  )}
                </article>
                <article className="debug-panel">
                  <h2>{t("debugger.variables")}</h2>
                  <Variables values={view.session?.variables ?? []} />
                  {!view.session?.variables.length && (
                    <p className="muted">{t("debugger.variablesEmpty")}</p>
                  )}
                </article>
              </div>
            </>
          ) : (
            <div className="debug-profiles">
              <article className="debug-panel">
                <h2>{t("debugger.history")}</h2>
                <ul>
                  {view.profiles.map((p) => (
                    <li key={p.id}>
                      <button
                        aria-pressed={p.id === selectedProfile}
                        onClick={() => void loadProfile(p.id)}
                      >
                        <span>{dateFormat.format(p.created_ms)}</span>
                        <span>{number(p.bytes)} B</span>
                      </button>
                    </li>
                  ))}
                </ul>
                {!view.profiles.length && (
                  <p className="muted">{t("debugger.profileEmpty")}</p>
                )}
              </article>
              <article className="debug-panel">
                <h2>{t("debugger.functions")}</h2>
                {profile ? (
                  <>
                    <p>
                      {t("debugger.total", {
                        time: number(profile.total_ms),
                        memory: number(profile.memory_bytes),
                      })}
                    </p>
                    <div className="workspace-toolbar">
                      <label>
                        {t("debugger.search")}
                        <input
                          name="profile-search"
                          value={query}
                          onChange={(e) => setQuery(e.target.value)}
                          autoComplete="off"
                        />
                      </label>
                      <button
                        onClick={() => void saveProfile()}
                        disabled={busy}
                      >
                        <Download aria-hidden="true" />
                        {t("debugger.export")}
                      </button>
                      <button
                        className="danger"
                        disabled={busy}
                        onClick={() => setRemove(selectedProfile)}
                      >
                        <Trash2 aria-hidden="true" />
                        {t("debugger.delete")}
                      </button>
                    </div>
                    <div
                      className="debug-function-table"
                      tabIndex={0}
                      role="region"
                      aria-label={t("debugger.functions")}
                    >
                      <table>
                        <thead>
                          <tr>
                            <th>{t("debugger.function")}</th>
                            <th>{t("debugger.selfTime")}</th>
                            <th>{t("debugger.inclusiveTime")}</th>
                            <th>{t("debugger.memory")}</th>
                            <th>{t("debugger.calls")}</th>
                          </tr>
                        </thead>
                        <tbody>
                          {values.map((f, index) => (
                            <tr key={f.file + f.name + String(index)}>
                              <td>
                                <code translate="no">{f.name}</code>
                                <small translate="no">{f.file}</small>
                              </td>
                              <td>{number(f.self_ms)} ms</td>
                              <td>{number(f.inclusive_ms)} ms</td>
                              <td>{number(f.memory_bytes)} B</td>
                              <td>{number(f.calls)}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                    {profile.truncated && (
                      <p className="muted">{t("debugger.truncated")}</p>
                    )}
                  </>
                ) : (
                  <p className="muted">{t("debugger.chooseProfile")}</p>
                )}
              </article>
            </div>
          )}
          <p className="muted">{t("debugger.privacy")}</p>
        </>
      )}
      {configure && view && (
        <WorkspaceModal
          title={t("debugger.configure")}
          busy={busy}
          onClose={() => setConfigure(false)}
          className="debug-dialog"
        >
          {errorNotice}
          <p>{t("debugger.configureHelp")}</p>
          <label>
            {t("debugger.mode")}
            <select
              name="debug-mode"
              value={draft.mode}
              onChange={(e) =>
                setDraft((s) => ({ ...s, mode: e.target.value as DebugMode }))
              }
            >
              {(["off", "debug", "profile"] as DebugMode[]).map((m) => (
                <option key={m} value={m}>
                  {t(`debugger.modes.${m}`)}
                </option>
              ))}
            </select>
          </label>
          <label>
            {t("debugger.runtime")}
            <select
              name="debug-runtime"
              value={draft.runtime ?? ""}
              onChange={(e) =>
                setDraft((s) => ({ ...s, runtime: e.target.value || null }))
              }
            >
              <option value="">{t("debugger.none")}</option>
              {view.runtimes
                .filter((r) => r.installed)
                .map((r) => (
                  <option key={r.id} value={r.id}>
                    PHP {r.php} / Xdebug {r.xdebug}
                  </option>
                ))}
            </select>
          </label>
          <label className="check-label">
            <input
              type="checkbox"
              name="debug-consent"
              checked={consent}
              onChange={(e) => setConsent(e.target.checked)}
            />
            {t("debugger.consent")}
          </label>
          {row?.state !== "stopped" && (
            <p className="notice">{t("debugger.stopFirst")}</p>
          )}
          <div className="modal-actions">
            <button disabled={busy} onClick={() => setConfigure(false)}>
              {t("work.cancel")}
            </button>
            <button
              className="primary"
              disabled={
                busy ||
                !consent ||
                row?.state !== "stopped" ||
                (draft.mode !== "off" && !draft.runtime)
              }
              onClick={() =>
                void execute({
                  action: "debug_configure",
                  id,
                  revision: row!.project.revision,
                  settings: draft,
                  consent,
                }).then((ok) => {
                  if (ok) setConfigure(false);
                })
              }
            >
              {t("work.save")}
            </button>
          </div>
        </WorkspaceModal>
      )}
      {installing && (
        <WorkspaceModal
          title={t("debugger.install")}
          busy={busy}
          onClose={() => setInstalling(false)}
          className="debug-dialog"
        >
          {errorNotice}
          <p>{t("debugger.buildHelp")}</p>
          <label className="check-label">
            <input
              type="checkbox"
              name="debug-build-trust"
              checked={consent}
              onChange={(e) => setConsent(e.target.checked)}
            />
            {t("debugger.trustBuild")}
          </label>
          {busy && (
            <div className="operation-progress">
              <p role="status">{t("debugger.building")}</p>
              <button onClick={() => void api.cancel()}>
                {t("setup.cancelOperation")}
              </button>
            </div>
          )}
          <div className="modal-actions">
            <button disabled={busy} onClick={() => setInstalling(false)}>
              {t("work.cancel")}
            </button>
            <button
              className="primary"
              disabled={busy || !consent}
              onClick={() =>
                void execute({
                  action: "debug_install",
                  id,
                  trusted: consent,
                }).then((ok) => {
                  if (ok) setInstalling(false);
                })
              }
            >
              {t("debugger.prepareRuntime")}
            </button>
          </div>
        </WorkspaceModal>
      )}
      {remove && (
        <WorkspaceModal
          title={t("debugger.delete")}
          busy={busy}
          onClose={() => setRemove(null)}
        >
          <p>{t("debugger.deleteHelp")}</p>
          {errorNotice}
          <div className="modal-actions">
            <button disabled={busy} onClick={() => setRemove(null)}>
              {t("work.cancel")}
            </button>
            <button
              className="danger"
              disabled={busy}
              onClick={() =>
                void execute({
                  action: "debug_profile_delete",
                  id,
                  profile: remove,
                  consent: true,
                }).then((ok) => {
                  if (ok) {
                    setRemove(null);
                    setProfile(null);
                    setSelectedProfile("");
                  }
                })
              }
            >
              {t("debugger.delete")}
            </button>
          </div>
        </WorkspaceModal>
      )}
    </section>
  );
}
