import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Activity,
  AlertCircle,
  CheckCircle2,
  Folder,
  Settings2,
} from "lucide-react";
import {
  desktopBackend,
  type Backend,
  type Bootstrap,
  type Preferences,
  type Theme,
} from "./bridge";
import { SettingsPage } from "./pages/SettingsPage";
import { ProjectsPage } from "./pages/ProjectsPage";
import { DiagnosticsPage } from "./pages/DiagnosticsPage";

type Page = "projects" | "settings" | "diagnostics";
const validPage = (value: string): Page =>
  ["projects", "settings", "diagnostics"].includes(value)
    ? (value as Page)
    : "settings";

export function App({ backend = desktopBackend }: { backend?: Backend }) {
  const { t, i18n } = useTranslation();
  const [page, setPage] = useState<Page>(() =>
    validPage(window.location.hash.slice(1)),
  );
  const [snapshot, setSnapshot] = useState<Bootstrap | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const mounted = useRef(true);
  const settings = snapshot?.settings ?? {
    language: "system",
    theme: "system" as Theme,
  };

  async function apply(value: Bootstrap) {
    await i18n.changeLanguage(value.resolvedLocale);
    if (!mounted.current) return;
    document.documentElement.lang = value.resolvedLocale;
    document.documentElement.dir = value.direction;
    document.documentElement.dataset.theme = value.settings.theme;
    setSnapshot(value);
  }
  const codeOf = (value: unknown) =>
    typeof value === "string" && i18n.exists(`errors.${value}`)
      ? value
      : "unexpected";
  async function load() {
    setBusy(true);
    setError(null);
    try {
      await apply(await backend.bootstrap());
    } catch (reason) {
      if (mounted.current) setError(codeOf(reason));
    } finally {
      if (mounted.current) {
        setLoading(false);
        setBusy(false);
      }
    }
  }
  async function persist(patch: Partial<Preferences>) {
    if (!snapshot || busy) return;
    setBusy(true);
    setError(null);
    try {
      await apply(await backend.save({ ...settings, ...patch }));
    } catch (reason) {
      if (mounted.current) setError(codeOf(reason));
    } finally {
      if (mounted.current) setBusy(false);
    }
  }
  useEffect(() => {
    mounted.current = true;
    void load();
    let stop: (() => void) | undefined;
    void backend
      .onNavigate((target) => {
        window.location.hash = validPage(target);
      })
      .then((unlisten) => {
        if (!mounted.current) unlisten();
        else stop = unlisten;
      })
      .catch(() => {});
    return () => {
      mounted.current = false;
      stop?.();
    };
  }, [backend]);
  useEffect(() => {
    const navigate = () => {
      setPage(validPage(window.location.hash.slice(1)));
      requestAnimationFrame(() => heading.current?.focus());
    };
    window.addEventListener("hashchange", navigate);
    return () => window.removeEventListener("hashchange", navigate);
  }, []);

  if (loading)
    return (
      <div className="boot" role="progressbar" aria-label="DevForge">
        <span className="brand-icon">D</span>
      </div>
    );

  const navigateItems = [
    { id: "projects", icon: Folder, group: "workspace" },
    { id: "settings", icon: Settings2, group: "application" },
    { id: "diagnostics", icon: Activity, group: "application" },
  ] as const;

  return (
    <div className="app-shell">
      <a
        className="skip-link"
        href="#main-content"
        onClick={(event) => {
          event.preventDefault();
          heading.current?.focus();
        }}
      >
        {t("common.skip")}
      </a>
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-icon" aria-hidden="true">
            D
          </span>
          <strong translate="no">DevForge</strong>
        </div>
        <nav aria-label={t("nav.main")}>
          {(["workspace", "application"] as const).map((group) => (
            <div className="nav-group" key={group}>
              <p className="nav-group-label">{t(`nav.${group}`)}</p>
              {navigateItems
                .filter((item) => item.group === group)
                .map(({ id, icon: Icon }) => (
                  <a
                    href={`#${id}`}
                    key={id}
                    className={`nav-item ${page === id ? "selected" : ""}`}
                    aria-current={page === id ? "page" : undefined}
                  >
                    <Icon aria-hidden="true" />
                    <span>{t(`nav.${id}`)}</span>
                  </a>
                ))}
            </div>
          ))}
        </nav>
        <div className="sidebar-version">
          v{snapshot?.diagnostics.appVersion ?? "0.1.0"}
        </div>
      </aside>
      <main id="main-content">
        <header className="page-header">
          <p className="breadcrumb">
            <span>{t("nav.application")}</span>
            <span aria-hidden="true">/</span>
            <span>
              {page === "settings"
                ? t("settings.breadcrumb")
                : t(`nav.${page}`)}
            </span>
          </p>
          <div className="title-row">
            <h1 ref={heading} tabIndex={-1}>
              {t(`${page}.title`)}
            </h1>
            <span
              className={`save-status ${snapshot ? "" : "unavailable"}`}
              role="status"
              aria-live="polite"
            >
              {snapshot ? (
                <CheckCircle2 aria-hidden="true" />
              ) : (
                <AlertCircle aria-hidden="true" />
              )}
              {t(
                !snapshot
                  ? "common.offline"
                  : busy
                    ? "common.saving"
                    : "common.saved",
              )}
            </span>
          </div>
          <p className="page-description">{t(`${page}.description`)}</p>
        </header>
        {error && (
          <div className="notice error" role="alert">
            <AlertCircle aria-hidden="true" />
            <p>{t(`errors.${error}`)}</p>
            <button onClick={() => void load()} disabled={busy}>
              {t("common.retry")}
            </button>
          </div>
        )}
        {snapshot && !snapshot.nativeUiSynced && (
          <div className="notice" role="alert">
            <AlertCircle aria-hidden="true" />
            <p>{t("errors.native_partial")}</p>
          </div>
        )}

        {page === "settings" && (
          <SettingsPage snapshot={snapshot} busy={busy} onChange={persist} />
        )}

        {page === "projects" && <ProjectsPage />}

        {page === "diagnostics" && (
          <DiagnosticsPage snapshot={snapshot} busy={busy} onRefresh={load} />
        )}
        <footer>
          DevForge <span aria-hidden="true">·</span> {t("common.footer")}
        </footer>
      </main>
    </div>
  );
}
