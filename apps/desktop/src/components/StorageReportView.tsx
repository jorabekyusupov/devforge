import { useId } from "react";
import { useTranslation } from "react-i18next";
import type {
  ReportPresentationState,
  RuntimeCleanupPreview,
  StorageInventory,
  StorageReference,
} from "./report-types";
import {
  ReportBytes,
  ReportFeedback,
  ReportProblems,
  ReportReason,
  TechnicalDetails,
} from "./ReportParts";

function References({ references }: { references: StorageReference[] }) {
  const { t } = useTranslation();
  return references.length ? (
    <ul>
      {references.map((ref) => (
        <li key={`${ref.kind}:${ref.id}`}>
          <code translate="no">{ref.kind}</code>
          {": "}
          <code translate="no">{ref.id}</code>
        </li>
      ))}
    </ul>
  ) : (
    <p>{t("storageView.noReferences")}</p>
  );
}
export function StorageReportView({
  inventory,
  preview = null,
  loading,
  errorCode,
  onRefresh,
}: ReportPresentationState & {
  inventory: StorageInventory | null;
  preview?: RuntimeCleanupPreview | null;
}) {
  const { t } = useTranslation();
  const heading = useId();
  const categories = new Set([
    "runtime_package",
    "database_instance",
    "database_generation",
    "cache_instance",
    "backups",
    "debug_profiles",
    "project_configuration",
    "project_logs",
    "mail",
    "worktree_payload",
    "workspace_metadata",
    "project_registry_metadata",
    "preferences_metadata",
    "inspection_artifacts",
    "tooling_logs",
    "restore_drill_receipt",
  ]);
  const valid = inventory?.schema === 1 ? inventory : null;
  const reviewed = preview?.schema === 1 ? preview : null;
  return (
    <section
      className="work-panel metadata-report"
      aria-labelledby={heading}
      aria-busy={loading}
    >
      <h2 id={heading}>{t("storageView.title")}</h2>
      <p>{t("storageView.sizeCaution")}</p>
      <p className="muted">{t("storageView.scopeCaution")}</p>
      <ReportFeedback
        loading={loading}
        errorCode={
          errorCode ??
          (inventory && !valid ? "report_schema_unsupported" : null)
        }
        onRefresh={onRefresh}
      />
      {!valid ? (
        !loading &&
        !errorCode && (
          <p>
            {t(inventory ? "reportView.unsupported" : "reportView.notLoaded")}
          </p>
        )
      ) : (
        <>
          {(loading || errorCode) && (
            <p className="muted">{t("reportView.previous")}</p>
          )}
          <dl>
            <dt>{t("storageView.logicalBytes")}</dt>
            <dd>
              <ReportBytes value={valid.logical_bytes_observed} />
            </dd>
            <dt>{t("storageView.measurement")}</dt>
            <dd>
              {t(
                valid.measurement_complete
                  ? "storageView.completeWithinScope"
                  : "storageView.incomplete",
              )}
            </dd>
            <dt>{t("storageView.registry")}</dt>
            <dd>
              {t(
                valid.registry_complete
                  ? "storageView.completeWithinScope"
                  : "storageView.incomplete",
              )}
            </dd>
          </dl>
          <ReportProblems codes={valid.problems} />
          {!valid.resources.length && <p>{t("storageView.noResources")}</p>}
          {valid.resources.map((resource) => (
            <article
              className="report-resource"
              key={`${resource.category}:${resource.resource_id}`}
            >
              <h3>
                {t(
                  categories.has(resource.category)
                    ? `storageView.categories.${resource.category}`
                    : "storageView.resource",
                )}
                {" · "}
                <code translate="no">{resource.resource_id}</code>
              </h3>
              <dl>
                <dt>{t("storageView.logicalBytes")}</dt>
                <dd>
                  <ReportBytes value={resource.logical_bytes} />
                </dd>
                <dt>{t("storageView.measurement")}</dt>
                <dd>
                  {t(
                    resource.measurement_complete
                      ? "storageView.completeWithinScope"
                      : "storageView.incomplete",
                  )}
                </dd>
                <dt>{t("storageView.retained")}</dt>
                <dd>
                  <ReportReason code={resource.retained_reason} />
                </dd>
                <dt>{t("storageView.ownership")}</dt>
                <dd>
                  {t(
                    resource.ownership === "unknown"
                      ? "storageView.ownershipUnknown"
                      : "storageView.ownershipRecorded",
                  )}
                </dd>
                <dt>{t("reportView.project")}</dt>
                <dd>
                  {resource.project_id ? (
                    <code translate="no">{resource.project_id}</code>
                  ) : (
                    t("reportView.unknown")
                  )}
                </dd>
                <dt>{t("storageView.workspace")}</dt>
                <dd>
                  {resource.workspace_id ? (
                    <code translate="no">{resource.workspace_id}</code>
                  ) : (
                    t("reportView.unknown")
                  )}
                </dd>
              </dl>
              <h4>{t("storageView.references")}</h4>
              <References references={resource.active_refs} />
              <p className="muted">{t("storageView.referenceCaution")}</p>
              <ReportProblems codes={resource.problems} />
              <TechnicalDetails
                codes={[
                  resource.category,
                  resource.ownership,
                  resource.reference_coverage,
                  resource.retained_reason,
                ]}
              />
            </article>
          ))}
          <TechnicalDetails
            codes={[
              valid.size_semantics,
              valid.scope_semantics,
              valid.snapshot_semantics,
            ]}
          />
        </>
      )}
      {preview && (
        <section className="report-resource">
          <h3>{t("storageView.preview")}</h3>
          <p>{t("storageView.previewCaution")}</p>
          {!reviewed ? (
            <p role="alert">{t("reportView.unsupported")}</p>
          ) : (
            <>
              <dl>
                <dt>{t("storageView.package")}</dt>
                <dd>
                  <code translate="no">{reviewed.package_id}</code>
                </dd>
                <dt>{t("storageView.reviewState")}</dt>
                <dd>{t(`storageView.previewState.${reviewed.state}`)}</dd>
                <dt>{t("storageView.lease")}</dt>
                <dd>{t(`storageView.leaseState.${reviewed.lease.state}`)}</dd>
                <dt>{t("storageView.logicalBytes")}</dt>
                <dd>
                  <ReportBytes value={reviewed.logical_bytes} />
                </dd>
              </dl>
              <p className="muted">{t("storageView.leaseCaution")}</p>
              <h4>{t("storageView.references")}</h4>
              <References references={reviewed.references} />
              <ReportProblems codes={reviewed.problems} />
              <TechnicalDetails
                codes={[
                  reviewed.action,
                  reviewed.descriptor_sha256,
                  reviewed.lease.reason,
                  reviewed.size_semantics,
                  reviewed.snapshot_semantics,
                  reviewed.apply_semantics,
                ]}
              />
            </>
          )}
        </section>
      )}
    </section>
  );
}
