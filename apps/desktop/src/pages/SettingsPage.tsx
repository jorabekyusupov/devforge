import { ProjectDiscoverySettings } from "../components/ProjectDiscoverySettings";
import type { Backend } from "../bridge";
import { useTranslation } from "react-i18next";
import { Globe, Monitor, Database } from "lucide-react";
import { registry } from "../i18n";
import type { Bootstrap, Preferences, Theme } from "../bridge";
export function SettingsPage({
  snapshot,
  busy,
  onChange,
  backend,
  onFolderChange,
}: {
  backend?: Backend;
  onFolderChange?: (path: string) => Promise<boolean>;
  snapshot: Bootstrap | null;
  busy: boolean;
  onChange: (patch: Partial<Preferences>) => Promise<void>;
}) {
  const { t, i18n } = useTranslation();
  const settings = snapshot?.settings ?? {
    language: "system",
    theme: "system" as Theme,
  };
  const languageName = (id: string, fallback: string) => {
    try {
      return (
        new Intl.DisplayNames([i18n.resolvedLanguage ?? registry.fallback], {
          type: "language",
        }).of(id) ?? fallback
      );
    } catch {
      return fallback;
    }
  };

  return (
    <div className="settings-content">
      <section className="card" aria-labelledby="language-heading">
        <div className="section-heading">
          <Globe aria-hidden="true" />
          <div>
            <h2 id="language-heading">{t("settings.languageTitle")}</h2>
            <p>{t("settings.languageDescription")}</p>
          </div>
        </div>
        <fieldset className="language-grid" disabled={busy || !snapshot}>
          <legend className="sr-only">{t("settings.languageTitle")}</legend>
          {registry.locales.map((locale) => (
            <label
              className={`language-tile ${snapshot?.resolvedLocale === locale.id ? "active" : ""}`}
              key={locale.id}
            >
              <input
                type="radio"
                name="language"
                value={locale.id}
                checked={snapshot?.resolvedLocale === locale.id}
                onChange={() => void onChange({ language: locale.id })}
                aria-label={locale.name}
              />
              <span className="radio-mark" aria-hidden="true" />
              <span className="language-copy">
                <strong lang={locale.id}>{locale.name}</strong>
                <span>{languageName(locale.id, locale.name)}</span>
              </span>
            </label>
          ))}
        </fieldset>
        <label className="system-language">
          <input
            type="checkbox"
            name="follow-system-language"
            checked={settings.language === "system"}
            disabled={busy || !snapshot}
            onChange={(event) =>
              void onChange({
                language: event.target.checked
                  ? "system"
                  : snapshot!.resolvedLocale,
              })
            }
          />
          <span>{t("settings.followSystem")}</span>
        </label>
        <p className="remembered">
          {t("settings.remembered")}{" "}
          <span className="sr-only">
            {t("settings.languageCount", {
              count: registry.locales.length,
            })}
          </span>
        </p>
        {snapshot?.unsupportedPreference && (
          <p className="notice" role="status">
            {t("settings.unsupported")}
          </p>
        )}
      </section>
      <section className="card" aria-labelledby="appearance-heading">
        <div className="section-heading">
          <Monitor aria-hidden="true" />
          <div>
            <h2 id="appearance-heading">{t("settings.appearanceTitle")}</h2>
            <p>{t("settings.appearanceDescription")}</p>
          </div>
        </div>
        <fieldset className="theme-grid" disabled={busy || !snapshot}>
          <legend className="sr-only">{t("settings.appearanceTitle")}</legend>
          {(["system", "light", "dark"] as const).map((theme) => (
            <label
              className={`theme-tile ${settings.theme === theme ? "active" : ""}`}
              key={theme}
            >
              <span
                className={`mini-window preview-${theme}`}
                aria-hidden="true"
              >
                <span className="mini-toolbar">
                  <i />
                  <i />
                  <i />
                </span>
                <span className="mini-lines">
                  <i />
                  <i />
                  <i />
                </span>
              </span>
              <span className="theme-label">
                <input
                  type="radio"
                  name="appearance"
                  value={theme}
                  checked={settings.theme === theme}
                  onChange={() => void onChange({ theme })}
                />
                <span>{t(`settings.${theme}`)}</span>
              </span>
            </label>
          ))}
        </fieldset>
      </section>
      <ProjectDiscoverySettings
        folder={settings.project_folder ?? "~/Devforge"}
        busy={busy || !snapshot}
        backend={backend}
        save={onFolderChange ?? (async () => false)}
      />
      <div className="local-storage">
        <Database aria-hidden="true" />
        <div>
          <strong>{t("common.storageTitle")}</strong>
          <p>{t("common.storageDescription")}</p>
        </div>
      </div>
    </div>
  );
}
