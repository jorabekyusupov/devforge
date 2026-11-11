import { useTranslation } from "react-i18next";
import { Activity, Database, RefreshCw } from "lucide-react";
import type { Bootstrap } from "../bridge";
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
  return (
    <div className="diagnostics-content">
      <section className="card">
        <div className="section-heading">
          <Activity aria-hidden="true" />
          <div>
            <h2>{t("diagnostics.device")}</h2>
            <p>{t("diagnostics.description")}</p>
          </div>
          <button
            className="refresh"
            onClick={() => void onRefresh()}
            disabled={busy}
          >
            <RefreshCw aria-hidden="true" />
            {t("common.refresh")}
          </button>
        </div>
        <dl className="diagnostic-rows">
          {(["operatingSystem", "architecture", "version"] as const).map(
            (key, index) => (
              <div key={key}>
                <dt>{t(`diagnostics.${key}`)}</dt>
                <dd>
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
            <dd>
              <code dir="ltr" translate="no">
                {snapshot?.diagnostics.databasePath ?? "—"}
              </code>
            </dd>
          </div>
        </dl>
      </section>
    </div>
  );
}
