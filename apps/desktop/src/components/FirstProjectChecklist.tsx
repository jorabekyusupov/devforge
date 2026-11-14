import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { Row, Snapshot } from "../workspace";
import "./FirstProjectChecklist.css";

type Status = "ready" | "needed" | "unknown" | "configured" | "manual";

/** A projection of saved/observed facts, never a substitute for an HTTP test. */
export function FirstProjectChecklist({
  row,
  snapshot,
  busy,
  dirty,
  onTool,
  openBrowser,
}: {
  row: Row;
  snapshot: Snapshot;
  busy: boolean;
  dirty: boolean;
  onTool: (kind: "prepare" | "php" | "https") => void;
  openBrowser: () => void;
}) {
  const { t } = useTranslation();
  const heading = useId();
  const project = row.project;
  const status = (value: Status) => (
    <span>{t(`firstProject.status.${value}`)}</span>
  );
  const installed = [project.php, project.node].every((id) =>
    snapshot.runtimes.some(
      (runtime) => runtime.id === id && runtime.installed && !runtime.problem,
    ),
  );
  const composer = project.composer
    ? snapshot.runtimes.find(
        (runtime) =>
          runtime.id === project.composer && runtime.kind === "composer",
      )
    : undefined;
  const composerStatus: Status = !project.composer
    ? "unknown"
    : composer?.installed && !composer.problem
      ? "ready"
      : "needed";
  const diagnostic = row.php_diagnostics;
  const prepared =
    diagnostic &&
    diagnostic.vendor_present &&
    diagnostic.environment_exists &&
    diagnostic.app_key_present;
  const optional: { key: string; status: Status; href: string }[] = [];
  if (
    row.database ||
    [project.services?.cache, project.services?.queue].includes("database")
  ) {
    optional.push({
      key: "database",
      status: row.database ? "configured" : "needed",
      href: "#services",
    });
  }
  if (row.development.queue) {
    optional.push({
      key: "queue",
      status:
        row.queue_state === "ready"
          ? "ready"
          : row.queue_state
            ? "needed"
            : "unknown",
      href: "#services",
    });
  }
  if (row.site?.vite) {
    optional.push({
      key: "vite",
      status: row.serving?.vite_ready ? "ready" : "needed",
      href: "#services",
    });
  }
  if (project.mail) {
    optional.push({
      key: "mail",
      status: "configured",
      href: `#activity?project=${encodeURIComponent(project.id)}`,
    });
  }
  return (
    <details
      className="work-panel first-project-checks"
      aria-labelledby={heading}
    >
      <summary id={heading}>{t("firstProject.title")}</summary>
      <p className="muted">{t("firstProject.hint")}</p>
      <ol>
        <li>
          <p>
            <strong>{t("firstProject.runtimes")}</strong>
            {" · "}
            {status(installed ? "ready" : "needed")}
          </p>
          <a href="#runtimes">{t("work.checkRuntimes")}</a>
        </li>
        <li>
          <p>
            <strong>{t("firstProject.composer")}</strong>
            {" · "}
            {status(composerStatus)}
          </p>
          <a href="#runtimes">{t("work.checkRuntimes")}</a>
        </li>
        <li>
          <p>
            <strong>{t("firstProject.files")}</strong>
            {" · "}
            {status(!diagnostic ? "unknown" : prepared ? "ready" : "needed")}
          </p>
          <p className="muted">{t("firstProject.preserve")}</p>
          <button
            type="button"
            disabled={busy || dirty}
            onClick={() => onTool(diagnostic ? "prepare" : "php")}
          >
            {t(
              diagnostic
                ? "firstProject.prepareAction"
                : "firstProject.phpAction",
            )}
          </button>{" "}
          <a href={`#configuration?project=${encodeURIComponent(project.id)}`}>
            {t("firstProject.secrets")}
          </a>
        </li>
        {optional.map((step) => (
          <li key={step.key}>
            <p>
              <strong>{t(`firstProject.${step.key}`)}</strong>
              {" · "}
              {status(step.status)}
            </p>
            <p className="muted">{t(`firstProject.${step.key}Hint`)}</p>
            <a href={step.href}>
              {t(step.key === "mail" ? "nav.activity" : "nav.services")}
            </a>
          </li>
        ))}
        <li>
          <p>
            <strong>{t("firstProject.http")}</strong>
            {" · "}
            {status("manual")}
          </p>
          <p className="muted">
            {t(
              row.serving?.php_ready
                ? "firstProject.httpReady"
                : "firstProject.httpStart",
            )}
          </p>
          <button
            type="button"
            disabled={busy || dirty || row.state !== "running"}
            onClick={openBrowser}
          >
            {t("firstProject.httpAction")}
          </button>
        </li>
        <li>
          <p>
            <strong>{t("firstProject.trace")}</strong>
            {" · "}
            {status("manual")}
          </p>
          <p className="muted">{t("firstProject.traceHint")}</p>
          <a href={`#inspector?project=${encodeURIComponent(project.id)}`}>
            {t("nav.inspector")}
          </a>
        </li>
      </ol>
    </details>
  );
}
