import { useTranslation } from "react-i18next";
import type { ProjectDetection, Snapshot } from "../workspace";

export const databaseNames = {
  mysql: "MySQL / MariaDB",
  postgresql: "PostgreSQL",
  sqlite: "SQLite",
  sqlsrv: "SQL Server",
};

export function matchingDatabases(
  database: ProjectDetection["database"],
  databases: Snapshot["databases"],
) {
  return databases.filter(({ instance }) =>
    instance.ready && instance.engine === database?.engine,
  );
}

export function ProjectDetectionSummary({
  detection,
  busy = false,
  install,
  metadataOnly = false,
}: {
  detection: ProjectDetection;
  busy?: boolean;
  install?: (id: string) => Promise<void>;
  metadataOnly?: boolean;
}) {
  const { t } = useTranslation();
  const runtime = (
    label: string,
    source: string,
    constraint: string,
    choice: ProjectDetection["php"],
  ) => (
    <li>
      <strong>{label}</strong>{" "}
      {label === "Node.js" && constraint === "*" ? (
        t("projectDetection.noNodeConstraint")
      ) : (
        <><code translate="no">{constraint}</code>{" · "}<code translate="no">{source}</code></>
      )}
      {" · "}
      {choice ? (
        <>
          <span translate="no">{choice.version}</span>{" "}
          {!choice.installed && install && (
            <button type="button" disabled={busy} onClick={() => void install(choice.id)}>
              {t("projectDetection.install", { runtime: label })}
            </button>
          )}
        </>
      ) : t("projectDetection.noRuntime")}
    </li>
  );
  return (
    <div className="detection-summary">
      <ul>
        {!metadataOnly && (
          <>
            {runtime("PHP", "composer.json", detection.php_constraint, detection.php)}
            {runtime("Node.js", "package.json", detection.node_constraint, detection.node)}
          </>
        )}
        <li>
          <strong>{t("projectDetection.extensions")}</strong>{" · "}
          {detection.extensions.length > 0 ? (
            <>
              <code translate="no">{detection.extensions.join(", ")}</code>
              {" · "}
              {detection.missing_extensions === null
                ? t("projectDetection.unchecked")
                : detection.missing_extensions.length
                  ? <>{t("projectDetection.missing")} <code translate="no">{detection.missing_extensions.join(", ")}</code></>
                  : t("projectDetection.extensionsReady")}
            </>
          ) : t("projectDetection.noExtensions")}
        </li>
        <li>
          <strong>{t("work.databaseService")}</strong>{" · "}
          {detection.database ? (
            <span translate="no">
              {databaseNames[detection.database.engine]} · <code>{detection.database.source}</code>
            </span>
          ) : t("projectDetection.noDatabase")}
        </li>
      </ul>
    </div>
  );
}
