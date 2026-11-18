import { useTranslation } from "react-i18next";
import type { ReportPresentationState } from "./report-types";
import "./ReportViews.css";

const knownReasons: Record<string, string> = {
  active_binding: "activeBinding",
  retained_package_other_process_leases_unknown: "runtimeLeases",
  retained_backup_payloads_not_validated: "retainedBackup",
  owned_workspace_or_retained_archive: "retainedWorkspace",
  pending_initialization_data_preserved: "pendingData",
  pending_generation_or_unverified_retained: "pendingData",
  owned_instance_data_preserved: "retainedData",
  owned_cache_data_preserved: "retainedData",
  retained_generation: "retainedData",
  retained_debug_artifacts: "retainedData",
  project_configuration_preserved: "retainedData",
  retained_logs: "retainedData",
  retained_mail: "retainedData",
  application_metadata_or_artifacts: "retainedData",
  drill_candidate_retained_requires_review: "drillReview",
  restore_drill_candidate_retained_requires_review: "drillReview",
  drill_generation_reference_unverified: "pendingData",
  matches_registered_metadata_not_runtime_verification: "metadataMatch",
  repository_and_registered_metadata_differ: "metadataDifference",
  declared_setting_not_registered: "missingSetting",
  no_explicit_setting_on_either_side: "notApplicable",
  exact_catalog_target_resolution_only: "catalogOnly",
  exact_runtime_family_unavailable_for_target: "targetUnavailable",
  credential_availability_not_queried_registered_name_is_metadata_only:
    "secretUnverified",
  secret_availability_not_queried: "secretUnverified",
  cached_php_diagnostics_without_timestamp_or_revision_not_current_verification:
    "cachedOnly",
  php_diagnostics_not_run: "notProbed",
  database_service_connection_dns_https_and_os_edition_not_probed: "notProbed",
  registry_snapshot_incomplete_absence_not_established: "registryIncomplete",
  reference_registry_incomplete: "registryIncomplete",
  active_registry_coverage_incomplete: "registryIncomplete",
  all_registered_reference_coverage_incomplete: "registryIncomplete",
};
function reasonKey(code: string): string {
  return Object.hasOwn(knownReasons, code) ? knownReasons[code] : "other";
}
export function ReportReason({ code }: { code: string }) {
  const { t } = useTranslation();
  return <span>{t(`reportView.reasons.${reasonKey(code)}`)}</span>;
}
export function TechnicalDetails({
  codes,
}: {
  codes: (string | null | undefined)[];
}) {
  const { t } = useTranslation();
  const values = [...new Set(codes.filter((code): code is string => !!code))];
  if (!values.length) return null;
  return (
    <details className="report-technical">
      <summary>{t("reportView.technical")}</summary>
      <ul>
        {values.map((code) => (
          <li key={code}>
            <code translate="no">{code}</code>
          </li>
        ))}
      </ul>
    </details>
  );
}
export function ReportProblems({ codes }: { codes: string[] }) {
  const { t } = useTranslation();
  if (!codes.length) return null;
  const primary = [...new Set(codes.map((code) => reasonKey(code)))];
  return (
    <div className="notice">
      <div>
        <strong>{t("reportView.limitations")}</strong>
        <ul>
          {primary.map((key) => (
            <li key={key}>{t(`reportView.reasons.${key}`)}</li>
          ))}
        </ul>
        <TechnicalDetails codes={codes} />
      </div>
    </div>
  );
}
export function ReportFeedback({
  loading,
  errorCode,
  onRefresh,
}: ReportPresentationState) {
  const { t } = useTranslation();
  return (
    <>
      {loading && <p role="status">{t("reportView.loading")}</p>}
      {errorCode && (
        <div role="alert" className="notice error">
          <div>
            {t("reportView.failed")}
            <TechnicalDetails codes={[errorCode]} />
          </div>
        </div>
      )}
      {onRefresh && (
        <button type="button" disabled={loading} onClick={onRefresh}>
          {t("reportView.refresh")}
        </button>
      )}
    </>
  );
}
export function ReportBytes({ value }: { value: number | null }) {
  const { t, i18n } = useTranslation();
  return (
    <>
      {value === null || !Number.isSafeInteger(value) || value < 0
        ? t("reportView.unknown")
        : new Intl.NumberFormat(i18n.resolvedLanguage ?? i18n.language, {
            style: "unit",
            unit: "byte",
            unitDisplay: "short",
          }).format(value)}
    </>
  );
}
