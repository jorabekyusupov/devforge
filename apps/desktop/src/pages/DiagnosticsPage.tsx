import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Activity, Database, RefreshCw, Copy, Check } from "lucide-react";
import type { Bootstrap } from "../bridge";
import "./DiagnosticsPage.css";

export function DiagnosticsPage({
  snapshot,
  busy,
  onRefresh,
}: {
  snapshot: Bootstrap | null;
  busy: boolean;
  onRefresh: () => Promise<void>;
}) {
  const { t, i18n } = useTranslation();
  const [copiedKey, setCopiedKey] = useState<"summary" | "path" | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, []);

  async function copyText(kind: "summary" | "path", value: string) {
    try {
      await navigator.clipboard.writeText(value);
      setCopiedKey(kind);
      if (timerRef.current) clearTimeout(timerRef.current);
      timerRef.current = setTimeout(() => setCopiedKey(null), 1800);
    } catch {
      setCopiedKey(null);
    }
  }

  function buildSummary() {
    if (!snapshot) return "";
    const d = snapshot.diagnostics;
    return [
      `DevForge ${d.appVersion}`,
      `OS: ${d.os} (${d.architecture})`,
      `Schema: ${d.schemaVersion}`,
      `Database: ${d.databasePath}`,
    ].join("\n");
  }

  return (
    <div className="diagnostics-content">
      <section className="card">
        <div className="section-heading">
          <Activity aria-hidden="true" />
          <div>
            <h2>{t("diagnostics.device")}</h2>
            <p>{t("diagnostics.description")}</p>
          </div>
          <div className="diagnostics-header-actions">
            <span
              className={`diagnostics-connection-pill ${snapshot ? "is-connected" : ""}`}
            >
              <i aria-hidden="true" />
              {snapshot
                ? t("diagnostics.connection")
                : t("common.offline")}
            </span>
            <button
              type="button"
              disabled={!snapshot}
              onClick={() => void copyText("summary", buildSummary())}
            >
              {copiedKey === "summary" ? (
                <Check aria-hidden="true" />
              ) : (
                <Copy aria-hidden="true" />
              )}
              {t(
                copiedKey === "summary"
                  ? "diagnostics.copied"
                  : "diagnostics.copySummary",
              )}
            </button>
            <button
              type="button"
              className="refresh"
              onClick={() => void onRefresh()}
              disabled={busy}
            >
              <RefreshCw aria-hidden="true" />
              {t("common.refresh")}
            </button>
          </div>
        </div>
        <dl className="diagnostics-device-grid">
          {(["operatingSystem", "architecture", "version"] as const).map(
            (key, index) => (
              <div key={key}>
                <dt>{t(`diagnostics.${key}`)}</dt>
                <dd translate="no">
                  {snapshot
                    ? [
                        snapshot.diagnostics.os,
                        snapshot.diagnostics.architecture,
                        snapshot.diagnostics.appVersion,
                      ][index]
                    : "—"}
                </dd>
              </div>
            ),
          )}
        </dl>
      </section>
      <section className="card">
        <div className="section-heading">
          <Database aria-hidden="true" />
          <div>
            <h2>{t("diagnostics.storage")}</h2>
            <p>{t("diagnostics.privacy")}</p>
          </div>
        </div>
        <dl className="diagnostic-rows">
          <div>
            <dt>{t("diagnostics.schema")}</dt>
            <dd>
              {snapshot
                ? new Intl.NumberFormat(i18n.resolvedLanguage).format(
                    snapshot.diagnostics.schemaVersion,
                  )
                : "—"}
            </dd>
          </div>
          <div className="path-row">
            <dt>{t("diagnostics.database")}</dt>
            <dd className="diagnostics-path-value">
              <code dir="ltr" translate="no">
                {snapshot?.diagnostics.databasePath ?? "—"}
              </code>
              {snapshot?.diagnostics.databasePath && (
                <button
                  type="button"
                  className="diagnostics-copy-btn"
                  onClick={() =>
                    void copyText("path", snapshot.diagnostics.databasePath)
                  }
                >
                  {copiedKey === "path" ? (
                    <Check aria-hidden="true" />
                  ) : (
                    <Copy aria-hidden="true" />
                  )}
                  {t(
                    copiedKey === "path"
                      ? "diagnostics.copied"
                      : "diagnostics.copyPath",
                  )}
                </button>
              )}
            </dd>
          </div>
        </dl>
      </section>
    </div>
  );
}
