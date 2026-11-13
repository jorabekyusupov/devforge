import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Mail,
  Search,
  Trash2,
  Download,
  Eye,
  AlertCircle,
  Radio,
} from "lucide-react";
import DOMPurify from "dompurify";
import { WorkspaceModal } from "../components/WorkspaceModal";
import {
  useWorkspace,
  type WorkspaceBackend,
  type Command,
  type MailView,
  type MailDetail,
  type MailPart,
  type LogPage,
  type LogFilter,
  type InspectionView,
} from "../workspace";
export function safeMailHtml(html: string) {
  const body = DOMPurify.sanitize(html, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: [
      "style",
      "form",
      "input",
      "button",
      "iframe",
      "object",
      "embed",
      "meta",
      "link",
      "base",
      "video",
      "audio",
    ],
    FORBID_ATTR: [
      "href",
      "src",
      "srcset",
      "action",
      "formaction",
      "target",
      "download",
    ],
  });
  return `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src 'none'; connect-src 'none'; base-uri 'none'; form-action 'none'"><meta name="color-scheme" content="light"><style>body{font:15px/1.6 system-ui,sans-serif;color:#1f2937;background:white;padding:20px;overflow-wrap:anywhere}img{max-width:100%}table{max-width:100%}</style></head><body>${body}</body></html>`;
}
export function ActivityPage({ backend }: { backend?: WorkspaceBackend }) {
  const { t, i18n } = useTranslation();
  const api = useWorkspace(backend);
  const [project, setProject] = useState(
    () => new URLSearchParams(location.hash.split("?")[1]).get("project") ?? "",
  );
  const projects = api.snapshot?.projects ?? [];
  const row = projects.find((r) => r.project.id === project) ?? projects[0];
  const id = row?.project.id ?? "";
  const [tab, setTab] = useState(() =>
    new URLSearchParams(location.hash.split("?")[1]).get("tab") === "logs"
      ? "logs"
      : "mail",
  );
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  const [selected, setSelected] = useState("");
  const [revision, setRevision] = useState(0);
  const [mail, setMail] = useState<MailView | null>(null);
  const [logs, setLogs] = useState<LogPage | null>(null);
  const [capture, setCapture] = useState<InspectionView | null>(null);
  const [detail, setDetail] = useState<MailDetail | null>(null);
  const [part, setPart] = useState<MailPart | null>(null);
  const [mode, setMode] = useState("html");
  const [failure, setFailure] = useState(false);
  const [detailFailure, setDetailFailure] = useState(false);
  const [confirm, setConfirm] = useState<Command | null>(null);
  const [level, setLevel] = useState("all");
  const [kind, setKind] = useState("all");
  const [hours, setHours] = useState("24");
  useEffect(() => {
    setMail(null);
    setLogs(null);
    setCapture(null);
  }, [id, tab]);
  useEffect(() => {
    setPage(0);
    setSelected("");
    setDetail(null);
    setPart(null);
    setFailure(false);
  }, [id, tab, query, level, kind, hours]);
  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        if (tab === "mail") {
          if (!backend?.inbox) throw Error();
          const value = await backend.inbox(id, query, page);
          if (alive) {
            setMail(value);
            setPage(value.inbox.page);
          }
        } else {
          if (!backend?.logs || !backend.inspector) throw Error();
          const filter: LogFilter = {
            query,
            page,
            kind,
            level,
            since: Date.now() - Number(hours) * 3600000,
          };
          const [value, meta] = await Promise.all([
            backend.logs(id, filter),
            backend.inspector(id),
          ]);
          if (alive) {
            setLogs(value);
            setPage(value.page);
            setCapture(meta);
          }
        }
        if (alive) setFailure(false);
      } catch {
        if (alive) setFailure(true);
      } finally {
        if (alive) timer = setTimeout(poll, 1500);
      }
    }
    if (id) void poll();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [id, tab, query, page, kind, level, hours, revision, backend]);
  const mailId =
    mail?.inbox.messages.find((m) => m.id === selected)?.id ??
    mail?.inbox.messages[0]?.id ??
    "";
  useEffect(() => {
    let alive = true;
    setDetail(null);
    setPart(null);
    setDetailFailure(false);
    if (id && tab === "mail" && mailId && backend?.message)
      void backend.message(id, mailId).then(
        (v) => {
          if (alive) {
            setDetail(v);
            setMode(v.html ? "html" : "text");
          }
        },
        () => {
          if (alive) setDetailFailure(true);
        },
      );
    return () => {
      alive = false;
    };
  }, [id, mailId, revision, tab, backend]);
  function navigate(next: string, nextProject = id) {
    setTab(next);
    setProject(nextProject);
    location.hash = `activity?project=${encodeURIComponent(nextProject)}&tab=${next}`;
  }
  async function execute(command: Command) {
    if (await api.run(command)) {
      setRevision((r) => r + 1);
      return true;
    }
    return false;
  }
  async function preview(index: number) {
    try {
      if (!backend?.part) throw Error();
      setPart(await backend.part(id, mailId, index));
    } catch {
      setDetailFailure(true);
    }
  }
  async function save(index: number, name: string) {
    try {
      const path = await backend?.chooseMailFile?.(name);
      if (path)
        await execute({
          action: "mail_save",
          id,
          message: mailId,
          index,
          path,
        });
    } catch {
      setDetailFailure(true);
    }
  }
  const log =
    logs?.entries.find((e) => String(e.sequence) === selected) ??
    logs?.entries[0];
  const date = (ms: number) =>
    new Intl.DateTimeFormat(i18n.language, {
      dateStyle: "short",
      timeStyle: "medium",
    }).format(new Date(ms));
  const number = (n: number) => new Intl.NumberFormat(i18n.language).format(n);
  const errorKey = api.error
    ? `${api.error.scope}.errors.${api.error.code}`
    : "";
  const total = tab === "mail" ? (mail?.inbox.total ?? 0) : (logs?.total ?? 0);
  const size = tab === "mail" ? 25 : 50;
  return (
    <section
      className="workspace-content activity-content"
      aria-busy={api.busy}
    >
      <div className="activity-project">
        <label>
          {t("activity.project")}
          <select
            name="activity-project"
            value={id}
            disabled={api.busy}
            onChange={(e) => navigate(tab, e.target.value)}
          >
            {projects.map((r) => (
              <option key={r.project.id} value={r.project.id}>
                {r.project.name}
              </option>
            ))}
          </select>
        </label>
      </div>
      {!id ? (
        <p>{t("inspector.noProjects")}</p>
      ) : (
        <>
          <nav className="activity-tabs" aria-label={t("activity.views")}>
            <button
              aria-pressed={tab === "mail"}
              onClick={() => navigate("mail")}
            >
              <Mail aria-hidden="true" />
              {t("activity.inbox")}
            </button>
            <button
              aria-pressed={tab === "logs"}
              onClick={() => navigate("logs")}
            >
              <Radio aria-hidden="true" />
              {t("activity.logs")}
            </button>
          </nav>
          {api.error && (
            <p role="alert" className="notice error">
              <AlertCircle aria-hidden="true" />
              {i18n.exists(errorKey) ? t(errorKey) : t("activity.failed")}
            </p>
          )}
          {api.busy && <p role="status">{t("work.working")}</p>}
          {api.result && !api.busy && (
            <p role="status" className="notice">
              {t("activity.completed")}
            </p>
          )}
          <div className="activity-toolbar">
            {tab === "mail" ? (
              <>
                <span className="mail-state">
                  {t(mail?.running ? "activity.running" : "activity.stopped")}{" "}
                  {mail?.port && <code>127.0.0.1:{mail.port}</code>}
                </span>
                <button
                  disabled={api.busy || row.state !== "stopped" || !mail}
                  onClick={() =>
                    void execute({
                      action: "mail_configure",
                      id,
                      enabled: !mail?.running,
                    })
                  }
                >
                  {t(mail?.running ? "activity.stop" : "activity.start")}
                </button>
                <button
                  disabled={api.busy || !total}
                  onClick={() =>
                    setConfirm({
                      action: "mail_delete",
                      id,
                      message: null,
                      consent: true,
                    })
                  }
                >
                  <Trash2 aria-hidden="true" />
                  {t("activity.clearInbox")}
                </button>
              </>
            ) : (
              <>
                <button
                  disabled={api.busy || row.state !== "stopped"}
                  onClick={() =>
                    setConfirm({ action: "inspector_install", id })
                  }
                >
                  <Download aria-hidden="true" />
                  {t("activity.installCapture")}
                </button>
                <button
                  disabled={api.busy || !capture?.companion_installed}
                  onClick={() =>
                    void execute({
                      action: "inspector_capture",
                      id,
                      enabled: !capture?.capture.enabled,
                    })
                  }
                >
                  {t(
                    capture?.capture.enabled
                      ? "inspector.pause"
                      : "inspector.enable",
                  )}
                </button>
                <button
                  disabled={api.busy || !capture?.capture.events.length}
                  onClick={() => setConfirm({ action: "inspector_clear", id })}
                >
                  {t("inspector.clear")}
                </button>
              </>
            )}
            <label className="activity-search">
              <span className="sr-only">{t("activity.search")}</span>
              <Search aria-hidden="true" />
              <input
                name="activity-search"
                type="search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t(
                  tab === "mail"
                    ? "activity.searchMail"
                    : "activity.searchLogs",
                )}
              />
            </label>
          </div>
          <p className="muted">
            {t(tab === "mail" ? "activity.mailHint" : "activity.logsHint")}
          </p>
          {tab === "logs" && (
            <div className="activity-filters">
              <label>
                {t("activity.level")}
                <select
                  name="log-level"
                  value={level}
                  onChange={(e) => setLevel(e.target.value)}
                >
                  {[
                    "all",
                    "debug",
                    "info",
                    "notice",
                    "warning",
                    "error",
                    "critical",
                    "alert",
                    "emergency",
                  ].map((l) => (
                    <option key={l} value={l}>
                      {t(`activity.levels.${l}`)}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                {t("activity.kind")}
                <select
                  name="log-kind"
                  value={kind}
                  onChange={(e) => setKind(e.target.value)}
                >
                  {["all", "log", "exception"].map((k) => (
                    <option key={k} value={k}>
                      {t(`activity.kinds.${k}`)}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                {t("activity.period")}
                <select
                  name="log-hours"
                  value={hours}
                  onChange={(e) => setHours(e.target.value)}
                >
                  {["1", "6", "24"].map((h) => (
                    <option value={h} key={h}>
                      {t("activity.hours", { count: Number(h) })}
                    </option>
                  ))}
                </select>
              </label>
            </div>
          )}
          {failure ? (
            <div role="alert" className="notice error">
              <p>{t("activity.failed")}</p>
              <button onClick={() => setRevision((v) => v + 1)}>
                {t("common.retry")}
              </button>
            </div>
          ) : !(tab==="mail"?mail:logs) ? <p role="status">{t("common.loading")}</p> : (
            <div className="activity-panels">
              <div className="activity-list">
                <p className="activity-count" role="status">
                  {t("activity.count", { count: total })}
                </p>
                <ul
                  aria-label={t(
                    tab === "mail" ? "activity.inbox" : "activity.logs",
                  )}
                >
                  {tab === "mail"
                    ? (mail?.inbox.messages ?? []).map((m) => (
                        <li key={m.id}>
                          <button
                            className="activity-item"
                            aria-pressed={m.id === mailId}
                            onClick={() => setSelected(m.id)}
                          >
                            <strong>
                              {m.subject || t("activity.noSubject")}
                            </strong>
                            <span>{m.from}</span>
                            <small>
                              {date(m.received_ms)}
                              {m.attachments
                                ? ` · ${t("activity.attachmentsCount", { count: m.attachments })}`
                                : ""}
                            </small>
                          </button>
                        </li>
                      ))
                    : (logs?.entries ?? []).map((e) => (
                        <li key={e.sequence}>
                          <button
                            className="activity-item"
                            aria-pressed={e.sequence === log?.sequence}
                            onClick={() => setSelected(String(e.sequence))}
                          >
                            <strong>
                              {e.kind === "exception"
                                ? e.name
                                : t(`activity.levels.${e.name}`, {
                                    defaultValue: e.name,
                                  })}
                            </strong>
                            <span>
                              {String(
                                (e.data as Record<string, unknown>)?.message ??
                                  "",
                              )}
                            </span>
                            <small>{date(e.received_ms)}</small>
                          </button>
                        </li>
                      ))}
                  {!total && (
                    <li className="activity-empty">
                      {t(
                        tab === "mail"
                          ? "activity.emptyMail"
                          : "activity.emptyLogs",
                      )}
                    </li>
                  )}
                </ul>
              </div>
              <article className="activity-detail">
                {tab === "mail" ? (
                  detail ? (
                    <>
                      <div className="activity-detail-title">
                        <h2>
                          {detail.summary.subject || t("activity.noSubject")}
                        </h2>
                        <button
                          disabled={api.busy}
                          onClick={() =>
                            setConfirm({
                              action: "mail_delete",
                              id,
                              message: mailId,
                              consent: true,
                            })
                          }
                        >
                          {t("activity.delete")}
                        </button>
                      </div>
                      <dl className="mail-addresses">
                        <dt>{t("activity.from")}</dt>
                        <dd>{detail.summary.from}</dd>
                        <dt>{t("activity.to")}</dt>
                        <dd>{detail.summary.to}</dd>
                      </dl>
                      <div className="activity-tabs">
                        <button
                          aria-pressed={mode === "html"}
                          disabled={!detail.html}
                          onClick={() => setMode("html")}
                        >
                          HTML
                        </button>
                        <button
                          aria-pressed={mode === "text"}
                          onClick={() => setMode("text")}
                        >
                          {t("activity.plainText")}
                        </button>
                      </div>
                      {mode === "html" ? (
                        <iframe
                          title={t("activity.htmlPreview")}
                          className="mail-html"
                          sandbox=""
                          referrerPolicy="no-referrer"
                          srcDoc={safeMailHtml(detail.html)}
                        />
                      ) : (
                        <pre className="mail-text" translate="no">
                          {detail.text || t("activity.noText")}
                        </pre>
                      )}
                      <p className="muted">
                        {t("activity.remoteBlocked")}
                        {detail.truncated && ` · ${t("activity.truncated")}`}
                      </p>
                      {!!detail.attachments.length && (
                        <section className="mail-attachments">
                          <h3>{t("activity.attachments")}</h3>
                          {detail.attachments.map((a) => (
                            <div className="mail-attachment" key={a.index}>
                              <span translate="no">{a.name}</span>
                              <small>{number(a.bytes)} B</small>
                              <button onClick={() => void preview(a.index)}>
                                <Eye aria-hidden="true" />
                                {t("activity.preview")}
                              </button>
                              <button
                                disabled={api.busy || !backend?.chooseMailFile}
                                onClick={() => void save(a.index, a.name)}
                              >
                                <Download aria-hidden="true" />
                                {t("activity.save")}
                              </button>
                            </div>
                          ))}
                        </section>
                      )}
                    </>
                  ) : (
                    <p>{t("activity.selectMail")}</p>
                  )
                ) : log ? (
                  <LogDetail event={log} id={id} />
                ) : (
                  <p>{t("activity.selectLog")}</p>
                )}
                {detailFailure && <p role="alert">{t("activity.failed")}</p>}
              </article>
            </div>
          )}
          {!!total && (
            <div className="activity-pager">
              <button disabled={!page} onClick={() => setPage((p) => p - 1)}>
                {t("activity.previous")}
              </button>
              <span>
                {number(page + 1)} / {number(Math.ceil(total / size))}
              </span>
              <button
                disabled={(page + 1) * size >= total}
                onClick={() => setPage((p) => p + 1)}
              >
                {t("activity.next")}
              </button>
            </div>
          )}
        </>
      )}
      {confirm && (
        <WorkspaceModal
          title={t("activity.confirm")}
          busy={api.busy}
          onClose={() => setConfirm(null)}
        >
          <p>
            {t(
              confirm.action === "inspector_install"
                ? "inspector.installHint"
                : "activity.deleteHint",
            )}
          </p>
          <div className="modal-actions">
            <button
              data-initial-focus
              disabled={api.busy}
              onClick={() => setConfirm(null)}
            >
              {t("work.cancel")}
            </button>
            <button
              disabled={api.busy}
              onClick={() =>
                void execute(confirm).then((ok) => {
                  if (ok) setConfirm(null);
                })
              }
            >
              {t("activity.continue")}
            </button>
          </div>
        </WorkspaceModal>
      )}
      {part && (
        <WorkspaceModal
          title={part.attachment.name}
          busy={false}
          onClose={() => setPart(null)}
        >
          <AttachmentPreview part={part} />
        </WorkspaceModal>
      )}
    </section>
  );
}
function AttachmentPreview({ part }: { part: MailPart }) {
  const { t } = useTranslation();
  if (
    ["image/png", "image/jpeg", "image/gif", "image/webp"].includes(
      part.attachment.mime,
    )
  )
    return (
      <img
        className="attachment-image"
        alt={part.attachment.name}
        src={`data:${part.attachment.mime};base64,${part.base64}`}
      />
    );
  if (
    part.attachment.mime.startsWith("text/") ||
    ["application/json", "application/xml"].includes(part.attachment.mime)
  ) {
    try {
      const data = Uint8Array.from(atob(part.base64), (c) => c.charCodeAt(0));
      return (
        <pre className="attachment-text" translate="no">
          {new TextDecoder().decode(data.slice(0, 65536))}
        </pre>
      );
    } catch {
      return <p>{t("activity.noPreview")}</p>;
    }
  }
  return <p>{t("activity.noPreview")}</p>;
}
function LogDetail({
  event,
  id,
}: {
  event: NonNullable<LogPage["entries"][number]>;
  id: string;
}) {
  const { t } = useTranslation();
  const data = (event.data ?? {}) as Record<string, unknown>;
  const frames = Array.isArray(data.trace)
    ? (data.trace
        .filter((v) => v !== null && typeof v === "object" && !Array.isArray(v))
        .slice(0, 40) as Record<string, unknown>[])
    : [];
  return (
    <>
      <h2>
        {event.kind === "exception"
          ? event.name
          : t(`activity.levels.${event.name}`, { defaultValue: event.name })}
      </h2>
      <pre className="log-message" translate="no">
        {String(data.message ?? "")}
      </pre>
      {data.file && (
        <p translate="no">
          {String(data.file)}:{String(data.line ?? "")}
        </p>
      )}
      <h3>{t("activity.context")}</h3>
      <code className="activity-context" translate="no">
        {event.context}
      </code>
      <a
        href={`#inspector?project=${encodeURIComponent(id)}&context=${event.context}`}
      >
        {t("activity.trace")}
      </a>
      <pre className="log-context" translate="no">
        {JSON.stringify(data.context ?? {}, null, 2)}
      </pre>
      {!!frames.length && (
        <section>
          <h3>{t("activity.stack")}</h3>
          <ol className="log-stack">
            {frames.map((f, i) => (
              <li key={i}>
                <code translate="no">
                  {String(f.function ?? "")} · {String(f.file ?? "")}:
                  {String(f.line ?? "")}
                </code>
              </li>
            ))}
          </ol>
        </section>
      )}
    </>
  );
}
