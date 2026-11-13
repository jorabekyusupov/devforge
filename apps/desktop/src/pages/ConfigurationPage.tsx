import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  useWorkspace,
  type WorkspaceBackend,
  type ConfigurationDocument,
  type ConfigurationPlan,
} from "../workspace";
import { WorkspaceModal } from "../components/WorkspaceModal";

/** Edits a portable file; the existing reviewed planner owns environment changes. */
export function ConfigurationPage({ backend }: { backend?: WorkspaceBackend }) {
  const { t } = useTranslation();
  const api = useWorkspace(backend);
  const [chosen, setChosen] = useState(
    () =>
      new URLSearchParams(window.location.hash.split("?")[1]).get("project") ??
      "",
  );
  const rows = api.snapshot?.projects ?? [];
  const row = rows.find((r) => r.project.id === chosen) ?? rows[0];
  const id = row?.project.id ?? "";
  const [doc, setDocument] = useState<ConfigurationDocument | null>(null);
  const [draft, setDraft] = useState("");
  const [plan, setPlan] = useState<ConfigurationPlan | null>(null);
  const [writeEnv, setWriteEnv] = useState(false);
  const [consent, setConsent] = useState(false);
  const [saving, setSaving] = useState(false);
  const [loading, setLoading] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [secretReference, setSecretReference] = useState("remote-password");
  const [secretValue, setSecretValue] = useState("");
  const [secretSaved, setSecretSaved] = useState(false);
  const ticket = useRef(0);
  const errorRef = useRef<HTMLDivElement>(null);
  const dirty = !!doc && draft !== doc.text;
  const busy = api.busy || loading;
  const stopped = row?.state === "stopped";
  useEffect(() => {
    const current = ++ticket.current;
    setDocument(null);
    setDraft("");
    setSecretValue("");
    setSecretSaved(false);
    setPlan(null);
    setFailure(null);
    setConsent(false);
    setSaving(false);
    if (!id || !backend?.configuration) return;
    setLoading(true);
    void backend
      .configuration(id, false, false)
      .then((result) => {
        if (current !== ticket.current) return;
        const doc = result as ConfigurationDocument;
        setDocument(doc);
        setDraft(doc.text);
      })
      .catch(() => {
        if (current === ticket.current)
          setFailure(t("configuration.readError"));
      })
      .finally(() => {
        if (current === ticket.current) setLoading(false);
      });
    return () => {
      ticket.current++;
    };
  }, [id, backend]);
  const error =
    failure ??
    (api.error
      ? t(`${api.error.scope}.errors.${api.error.code}`, {
          defaultValue: t("errors.unexpected"),
        })
      : null);
  useEffect(() => {
    if (!dirty) return;
    const unload = (e: BeforeUnloadEvent) => {
      e.preventDefault();
      e.returnValue = "";
    };
    const navigate = (e: MouseEvent) => {
      const a = (e.target as Element).closest?.("a[href]");
      if (a && (a as HTMLAnchorElement).hash !== "#configuration") {
        e.preventDefault();
        setFailure(t("configuration.unsaved"));
      }
    };
    window.addEventListener("beforeunload", unload);
    window.document.addEventListener("click", navigate, true);
    return () => {
      window.removeEventListener("beforeunload", unload);
      window.document.removeEventListener("click", navigate, true);
    };
  }, [dirty, t]);
  useEffect(() => {
    if (error) errorRef.current?.focus();
  }, [error]);
  async function review() {
    if (!backend?.configuration) return;
    setLoading(true);
    setFailure(null);
    setPlan(null);
    setConsent(false);
    try {
      setPlan(
        (await backend.configuration(id, true, writeEnv)) as ConfigurationPlan,
      );
    } catch {
      setFailure(t("configuration.reviewError"));
    } finally {
      setLoading(false);
    }
  }
  return (
    <section
      className="workspace-content configuration-content"
      aria-label={t("configuration.title")}
    >
      <div className="workspace-toolbar configuration-toolbar">
        <label>
          {t("configuration.project")}
          <select
            name="configuration-project"
            disabled={busy}
            value={id}
            onChange={(e) => {
              if (!dirty) {
                setChosen(e.target.value);
              } else {
                setFailure(t("configuration.unsaved"));
              }
            }}
          >
            {rows.map((r) => (
              <option key={r.project.id} value={r.project.id}>
                {r.project.name}
              </option>
            ))}
          </select>
        </label>
      </div>
      {error && (
        <div className="notice error" tabIndex={-1} ref={errorRef} role="alert">
          {error}
        </div>
      )}
      {!rows.length ? (
        <div className="empty-state">
          <h2>{t("configuration.empty")}</h2>
        </div>
      ) : loading && !doc ? (
        <p role="status">{t("configuration.loading")}</p>
      ) : (
        doc && (
          <div className="configuration-grid">
            <article className="debug-panel">
              <div className="configuration-file">
                <h2 translate="no">devforge.yaml</h2>
                <span role="status">
                  {t(dirty ? "configuration.modified" : "configuration.saved")}
                </span>
              </div>
              <label htmlFor="configuration-yaml">
                {t("configuration.fileLabel")}
              </label>
              <textarea
                id="configuration-yaml"
                name="configuration-yaml"
                className="configuration-editor mono"
                value={draft}
                maxLength={65536}
                onChange={(e) => {
                  setDraft(e.target.value);
                  setPlan(null);
                }}
                disabled={busy}
                spellCheck={false}
                autoComplete="off"
                aria-describedby="configuration-help"
                translate="no"
              />
              <p id="configuration-help" className="muted">
                {t("configuration.help")}
              </p>
              <div className="modal-actions">
                <button
                  disabled={busy || !stopped || (!dirty && doc.exists)}
                  onClick={() => {
                    setSaving(true);
                    setConsent(false);
                    api.clearError();
                  }}
                >
                  {t("configuration.save")}
                </button>
                <button
                  disabled={busy || !dirty}
                  onClick={() => {
                    setDraft(doc.text);
                    setFailure(null);
                  }}
                >
                  {t("configuration.discard")}
                </button>
              </div>
            </article>
            <article className="debug-panel configuration-review">
              <h2>{t("configuration.review")}</h2>
              <p>{t("configuration.reviewHelp")}</p>
              {!stopped && (
                <p className="notice">{t("configuration.stopFirst")}</p>
              )}
              <label className="check-label">
                <input
                  type="checkbox"
                  checked={writeEnv}
                  disabled={busy}
                  onChange={(e) => {
                    setWriteEnv(e.target.checked);
                    setPlan(null);
                  }}
                />
                {t("configuration.writeEnv")}
              </label>
              <button
                disabled={busy || dirty || !doc.exists || !stopped}
                onClick={() => void review()}
              >
                {t("configuration.reviewAction")}
              </button>
              {plan && (
                <>
                  <h3>{t("configuration.downloads")}</h3>
                  {!plan.downloads.length && (
                    <p className="muted">{t("forks.none")}</p>
                  )}
                  <ul>
                    {plan.downloads.map((p) => (
                      <li key={p} translate="no">
                        {p}
                      </li>
                    ))}
                  </ul>
                  <p>
                    <strong>{t("configuration.database")}: </strong>
                    <span>
                      {t(
                        `configuration.databaseActions.${plan.database_action}`,
                      )}
                    </span>
                  </p>
                  {plan.secrets.map((s) => (
                    <p key={s.environment}>
                      <code>{s.environment}</code>:{" "}
                      {t(
                        s.available
                          ? "configuration.secretReady"
                          : "configuration.secretMissing",
                      )}
                    </p>
                  ))}
                  {!!plan.blockers.length && (
                    <div className="notice error" role="alert">
                      <h3>{t("configuration.blockers")}</h3>
                      <ul>
                        {plan.blockers.map((b, i) => (
                          <li key={i}>
                            {t(`workspace.errors.${b}`, {
                              defaultValue: t("configuration.reviewError"),
                            })}
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}
                  <label className="check-label">
                    <input
                      type="checkbox"
                      checked={consent}
                      onChange={(e) => setConsent(e.target.checked)}
                    />
                    {t("configuration.trust")}
                  </label>
                  <button
                    className="primary"
                    disabled={busy || !consent || !!plan.blockers.length}
                    onClick={() =>
                      void api
                        .run({
                          action: "configuration_apply",
                          id,
                          review: plan.digest,
                          trusted: true,
                          write_env: writeEnv,
                        })
                        .then(async (ok) => {
                          if (ok) {
                            if (backend?.configuration) {
                              const fresh = (await backend.configuration(
                                id,
                                false,
                                false,
                              )) as ConfigurationDocument;
                              setDocument(fresh);
                              setDraft(fresh.text);
                            }
                            setPlan(null);
                            setConsent(false);
                            setFailure(null);
                          }
                        })
                        .catch(() => setFailure(t("configuration.readError")))
                    }
                  >
                    {t("configuration.apply")}
                  </button>
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
              <details>
                <summary>{t("configuration.localSecret")}</summary>
                <form
                  onSubmit={(e) => {
                    e.preventDefault();
                    if (!backend?.configurationSecret) return;
                    setLoading(true);
                    void backend
                      .configurationSecret(id, secretReference, secretValue)
                      .then(() => {
                        setSecretValue("");
                        setSecretSaved(true);
                        setPlan(null);
                      })
                      .catch(() => setFailure(t("configuration.secretError")))
                      .finally(() => setLoading(false));
                  }}
                >
                  <label>
                    {t("configuration.secretReference")}
                    <input
                      name="secret-reference"
                      value={secretReference}
                      onChange={(e) => {
                        setSecretReference(e.target.value);
                        setSecretSaved(false);
                      }}
                      required
                      maxLength={64}
                      pattern="[a-z0-9_-]+"
                      autoComplete="off"
                    />
                  </label>
                  <label>
                    {t("configuration.secretValue")}
                    <input
                      name="secret-value"
                      type="password"
                      value={secretValue}
                      onChange={(e) => {
                        setSecretValue(e.target.value);
                        setSecretSaved(false);
                      }}
                      required
                      maxLength={4096}
                      autoComplete="new-password"
                    />
                  </label>
                  <button
                    disabled={busy || !backend?.configurationSecret}
                    type="submit"
                  >
                    {t("configuration.storeSecret")}
                  </button>
                  {secretSaved && (
                    <p role="status">{t("configuration.secretReady")}</p>
                  )}
                </form>
              </details>
              {doc.journal && (
                <p role="status">
                  {t("configuration.journal")}:{" "}
                  <span>
                    {t(`configuration.stages.${doc.journal.stage}`, {
                      defaultValue: t("configuration.loading"),
                    })}
                  </span>
                </p>
              )}
            </article>
          </div>
        )
      )}
      {saving && doc && (
        <WorkspaceModal
          busy={api.busy}
          title={t("configuration.save")}
          onClose={() => {
            if (!api.busy) setSaving(false);
          }}
        >
          {error && (
            <p className="notice error" role="alert">
              {error}
            </p>
          )}
          <p>{t("configuration.saveHelp")}</p>
          <label className="check-label">
            <input
              type="checkbox"
              checked={consent}
              onChange={(e) => setConsent(e.target.checked)}
            />
            {t("configuration.saveConsent")}
          </label>
          <div className="modal-actions">
            <button disabled={api.busy} onClick={() => setSaving(false)}>
              {t("work.cancel")}
            </button>
            <button
              className="primary"
              disabled={api.busy || !consent}
              onClick={() =>
                void api.run(
                  {
                    action: "configuration_save",
                    id,
                    expected: doc.expected,
                    text: draft,
                    consent: true,
                  },
                  (value) => {
                    const doc = value as unknown as ConfigurationDocument;
                    setDocument(doc);
                    setDraft(doc.text);
                    setSaving(false);
                    setPlan(null);
                  },
                )
              }
            >
              {t("configuration.save")}
            </button>
          </div>
        </WorkspaceModal>
      )}
    </section>
  );
}
