import { useEffect, useRef, useState } from "react";
import { Folder, RefreshCw } from "lucide-react";
import { WorkspaceModal } from "./WorkspaceModal";
import { useTranslation } from "react-i18next";
import type { Backend } from "../bridge";
import { useWorkspace } from "../workspace";

export function ProjectDiscoverySettings({
  folder,
  busy,
  backend,
  save,
}: {
  folder: string;
  busy: boolean;
  backend?: Backend;
  save: (path: string) => Promise<boolean>;
}) {
  const { t, i18n } = useTranslation();
  const api = useWorkspace(backend?.workspace);
  const input = useRef<HTMLInputElement>(null);
  const browseButton = useRef<HTMLButtonElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);
  const [leaveTo, setLeaveTo] = useState<string | null>(null);
  const [draft, setDraft] = useState(folder);
  const acknowledged = useRef(folder);
  const dirtyRef = useRef(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    setDraft(folder);
    acknowledged.current = folder;
  }, [folder]);
  const dirty = draft !== folder && draft !== acknowledged.current;
  dirtyRef.current = dirty;
  useEffect(() => {
    if (!dirty) return;
    const guard = (event: MouseEvent) => {
      if (!dirtyRef.current) return;
      const link = (event.target as Element).closest<HTMLAnchorElement>(
        "a[href]",
      );
      if (link?.hash && link.hash !== "#settings") {
        event.preventDefault();
        setLeaveTo(link.hash);
      }
    };
    const unload = (event: BeforeUnloadEvent) => event.preventDefault();
    document.addEventListener("click", guard, true);
    window.addEventListener("beforeunload", unload);
    return () => {
      document.removeEventListener("click", guard, true);
      window.removeEventListener("beforeunload", unload);
    };
  }, [dirty]);
  const working = busy || pending;
  const report = api.snapshot?.discovery;
  async function perform(action: () => Promise<void>) {
    if (working) return;
    setPending(true);
    setError("");
    try {
      await action();
    } catch (reason) {
      const code =
        typeof reason === "string" ? reason : "project_folder_unavailable";
      setError(
        i18n.exists(`errors.${code}`)
          ? t(`errors.${code}`)
          : t("discovery.failed"),
      );
    } finally {
      setPending(false);
      const target = returnFocus.current;
      returnFocus.current = null;
      if (target)
        setTimeout(() => {
          if (target.isConnected) target.focus();
        }, 0);
    }
  }
  return (
    <section
      className="card discovery-settings"
      aria-labelledby="discovery-heading"
      aria-busy={working}
    >
      <div className="section-heading">
        <Folder aria-hidden="true" />
        <div>
          <h2 id="discovery-heading">{t("discovery.title")}</h2>
          <p id="project-folder-hint">{t("discovery.description")}</p>
        </div>
      </div>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void perform(async () => {
            if (!draft.trim() || /[\u0000-\u001f\u007f]/.test(draft)) {
              setError(t("errors.invalid_project_folder"));
              returnFocus.current = input.current;
              return;
            }
            if (!(await save(draft))) {
              setError(t("discovery.failed"));
              returnFocus.current = input.current;
            } else {
              acknowledged.current = draft;
              dirtyRef.current = false;
            }
          });
        }}
      >
        <label htmlFor="project-folder">{t("discovery.folder")}</label>
        <div className="discovery-path">
          <input
            ref={input}
            id="project-folder"
            name="project-folder"
            value={draft}
            autoComplete="off"
            spellCheck={false}
            aria-describedby={
              error
                ? "project-folder-hint project-folder-error"
                : "project-folder-hint"
            }
            aria-invalid={!!error}
            disabled={working}
            onChange={(e) => {
              setDraft(e.target.value);
              setError("");
            }}
          />
          <button
            ref={browseButton}
            type="button"
            disabled={working || !backend?.workspace}
            onClick={() =>
              void perform(async () => {
                const chosen = await backend?.workspace?.browse();
                if (chosen) {
                  setDraft(chosen);
                  returnFocus.current = input.current;
                } else {
                  returnFocus.current = browseButton.current;
                }
              })
            }
          >
            {t("discovery.browse")}
          </button>
        </div>
        {error && (
          <p id="project-folder-error" className="notice error" role="alert">
            {error}
          </p>
        )}
        <div className="discovery-actions">
          <button
            type="submit"
            className="primary"
            disabled={working || !draft.trim() || draft === folder}
          >
            {t("discovery.save")}
          </button>
          <button
            type="button"
            disabled={working || !backend?.createFolder || !draft.trim()}
            onClick={() =>
              void perform(async () => {
                const path = await backend!.createFolder!(draft);
                setDraft(path);
              })
            }
          >
            {t("discovery.create")}
          </button>
          <button
            type="button"
            disabled={working}
            onClick={() => {
              setDraft("~/Devforge");
              setError("");
            }}
          >
            {t("discovery.reset")}
          </button>
          <button
            type="button"
            disabled={working || !backend?.scan || report?.state === "scanning"}
            onClick={() =>
              void perform(async () => {
                await backend!.scan!();
                await api.refresh();
              })
            }
          >
            <RefreshCw aria-hidden="true" />
            {t("discovery.scan")}
          </button>
        </div>
      </form>
      <p className="muted">{t("discovery.defaultHint")}</p>
      {report && (
        <div className="discovery-status" role="status" aria-live="polite">
          <strong>{t(`discovery.states.${report.state || "scanning"}`)}</strong>
          <span>
            {t("discovery.count", {
              count: report.entries.filter(
                (e) =>
                  !e.ignored && e.state !== "detached" && e.state !== "missing",
              ).length,
            })}
          </span>
          {report.last_scan > 0 && (
            <span>
              {t("discovery.lastScan", {
                time: new Intl.DateTimeFormat(i18n.language, {
                  hour: "2-digit",
                  minute: "2-digit",
                  second: "2-digit",
                }).format(report.last_scan),
              })}
            </span>
          )}
          {report.problem && (
            <span>
              {t(`discovery.problems.${report.problem}`, {
                defaultValue: t("discovery.failed"),
              })}
            </span>
          )}
        </div>
      )}
      {leaveTo && (
        <WorkspaceModal
          title={t("discovery.unsaved")}
          busy={working}
          onClose={() => setLeaveTo(null)}
        >
          <p>{t("discovery.unsavedDescription")}</p>
          <div className="discovery-actions">
            <button data-initial-focus onClick={() => setLeaveTo(null)}>
              {t("work.keepEditing")}
            </button>
            <button
              onClick={() => {
                const target = leaveTo;
                setLeaveTo(null);
                setDraft(folder);
                location.hash = target;
              }}
            >
              {t("work.discard")}
            </button>
          </div>
        </WorkspaceModal>
      )}
    </section>
  );
}
