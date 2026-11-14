import { useId } from "react";
import { useTranslation } from "react-i18next";
import type {
  EnvironmentReport,
  ReportPresentationState,
} from "./report-types";
import {
  ReportFeedback,
  ReportProblems,
  ReportReason,
  TechnicalDetails,
} from "./ReportParts";

export function EnvironmentReportView({
  report,
  loading,
  errorCode,
  onRefresh,
}: ReportPresentationState & { report: EnvironmentReport | null }) {
  const { t } = useTranslation();
  const heading = useId();
  const valid = report?.schema === 1 ? report : null;
  const codes = valid
    ? [
        valid.scope,
        valid.snapshot_semantics,
        ...valid.comparisons.flatMap((fact) => [
          fact.key,
          fact.reason,
          fact.next_action,
        ]),
        ...valid.portability.flatMap((target) => [target.scope, target.reason]),
        ...valid.secrets.map((secret) => secret.reason),
        ...valid.extensions.map((extension) => extension.evidence),
      ]
    : [];
  return (
    <section
      className="work-panel metadata-report"
      aria-labelledby={heading}
      aria-busy={loading}
    >
      <h2 id={heading}>{t("environmentView.title")}</h2>
      <p>{t("environmentView.caution")}</p>
      <ReportFeedback
        loading={loading}
        errorCode={
          errorCode ?? (report && !valid ? "report_schema_unsupported" : null)
        }
        onRefresh={onRefresh}
      />
      {!valid ? (
        !loading &&
        !errorCode && (
          <p>{t(report ? "reportView.unsupported" : "reportView.notLoaded")}</p>
        )
      ) : (
        <>
          {(loading || errorCode) && (
            <p className="muted">{t("reportView.previous")}</p>
          )}
          <dl>
            <dt>{t("reportView.project")}</dt>
            <dd>
              <code translate="no">{valid.project_id}</code>
            </dd>
            <dt>{t("environmentView.metadataMatch")}</dt>
            <dd>
              {t(
                `environmentView.match.${valid.matches_recorded_configuration === null ? "unknown" : valid.matches_recorded_configuration ? "yes" : "no"}`,
              )}
            </dd>
            <dt>{t("environmentView.health")}</dt>
            <dd>{t("environmentView.healthUnknown")}</dd>
          </dl>
          <ReportProblems codes={valid.problems} />
          <table>
            <caption>{t("environmentView.comparisons")}</caption>
            <thead>
              <tr>
                <th scope="col">{t("environmentView.setting")}</th>
                <th scope="col">{t("environmentView.expected")}</th>
                <th scope="col">{t("environmentView.actual")}</th>
                <th scope="col">{t("reportView.evidence")}</th>
              </tr>
            </thead>
            <tbody>
              {valid.comparisons.map((fact) => (
                <tr key={fact.key}>
                  <th scope="row">
                    <code translate="no">{fact.key}</code>
                  </th>
                  <td>
                    {fact.expected === null ? (
                      t("reportView.unknown")
                    ) : (
                      <code translate="no">{fact.expected}</code>
                    )}
                  </td>
                  <td>
                    {fact.actual === null ? (
                      t("reportView.unknown")
                    ) : (
                      <code translate="no">{fact.actual}</code>
                    )}
                  </td>
                  <td>
                    <strong>{t(`reportView.state.${fact.state}`)}</strong>
                    <p>
                      <ReportReason code={fact.reason} />
                    </p>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {!valid.comparisons.length && <p>{t("environmentView.noFacts")}</p>}
          <table>
            <caption>{t("environmentView.portability")}</caption>
            <thead>
              <tr>
                <th scope="col">{t("environmentView.target")}</th>
                <th scope="col">{t("environmentView.catalog")}</th>
                <th scope="col">{t("environmentView.native")}</th>
              </tr>
            </thead>
            <tbody>
              {valid.portability.map((item) => (
                <tr key={item.target}>
                  <th scope="row">
                    <code translate="no">{item.target}</code>
                  </th>
                  <td>{t(`reportView.state.${item.catalog_support}`)}</td>
                  <td>{t(`reportView.state.${item.native_support}`)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <h3>{t("environmentView.secrets")}</h3>
          <p className="muted">{t("environmentView.secretHint")}</p>
          {!valid.secrets.length ? (
            <p>{t("environmentView.noSecretFacts")}</p>
          ) : (
            <ul>
              {valid.secrets.map((secret) => (
                <li key={secret.environment}>
                  <code translate="no">{secret.environment}</code>
                  {" → "}
                  <code translate="no">{secret.reference_alias}</code>
                  {" · "}
                  {t(
                    secret.registered_name
                      ? "environmentView.nameRegistered"
                      : "environmentView.nameMissing",
                  )}
                  {" · "}
                  {t(`reportView.state.${secret.availability}`)}
                </li>
              ))}
            </ul>
          )}
          <h3>{t("environmentView.extensions")}</h3>
          <p className="muted">{t("environmentView.extensionHint")}</p>
          {!valid.extensions.length ? (
            <p>{t("environmentView.noExtensionFacts")}</p>
          ) : (
            <ul>
              {valid.extensions.map((extension) => (
                <li key={extension.extension}>
                  <code translate="no">{extension.extension}</code>
                  {" · "}
                  {t(
                    `environmentView.cached.${extension.cached_present === null ? "unknown" : extension.cached_present ? "present" : "absent"}`,
                  )}
                  {" · "}
                  {t(`reportView.state.${extension.current_support}`)}
                </li>
              ))}
            </ul>
          )}
          <details>
            <summary>{t("environmentView.artifacts")}</summary>
            <p>{t("environmentView.artifactHint")}</p>
            <ul>
              {valid.artifact_evidence.requirements.map((item) => (
                <li key={`${item.role}:${item.package_id}`}>
                  <code translate="no">{item.role}</code>
                  {": "}
                  <code translate="no">
                    {item.package_id ?? t("reportView.unknown")}
                  </code>
                  {" · "}
                  {t(`reportView.state.${item.state}`)}
                  {item.superseded_by && (
                    <p>
                      {t("environmentView.replacement")}
                      {": "}
                      <code translate="no">{item.superseded_by}</code>
                    </p>
                  )}
                </li>
              ))}
            </ul>
            <ul>
              {valid.artifact_evidence.prerequisites.map((item) => (
                <li key={item.code}>
                  <ReportReason code={item.reason} />
                  {" · "}
                  {t(`reportView.state.${item.state}`)}
                </li>
              ))}
            </ul>
            <ReportProblems codes={valid.artifact_evidence.problems} />
            <TechnicalDetails
              codes={[
                valid.artifact_evidence.scope,
                valid.artifact_evidence.snapshot_semantics,
                valid.artifact_evidence.catalog_provenance,
                ...valid.artifact_evidence.requirements.flatMap((item) => [
                  item.selection_basis,
                  item.reason,
                  item.verification,
                ]),
                ...valid.artifact_evidence.prerequisites.flatMap((item) => [
                  item.code,
                  item.reason,
                ]),
              ]}
            />
          </details>
          <TechnicalDetails codes={codes} />
        </>
      )}
    </section>
  );
}
