import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link2 } from "lucide-react";
import type {
  AgentApi,
  AgentSnapshot,
  AgentGrant,
  AgentPlan,
  AgentScope,
  AgentConfig,
  AgentLookupResult,
} from "../agent-api";
import { WorkspaceModal } from "./WorkspaceModal";
import "./AgentConnections.css";

const scopes: AgentScope[] = [
  "observer",
  "project_manage",
  "execution_test",
  "sensitive_operation",
];
type Dialog =
  | { kind: "revoke"; grant: AgentGrant }
  | { kind: "approve" | "domain"; plan: AgentPlan };

export function AgentConnections({ api }: { api?: AgentApi }) {
  const { t, i18n } = useTranslation();
  const heading = useId();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const focusAfterAction = useRef(false);
  const [view, setView] = useState<AgentSnapshot | null>(null);
  const [pending, setPending] = useState(false);
  const [stale, setStale] = useState(false);
  const [error, setError] = useState(false);
  const [message, setMessage] = useState<"done" | "grantCreated" | null>(null);
  const [project, setProject] = useState("");
  const [selectedScopes, setSelectedScopes] = useState<AgentScope[]>([
    "observer",
  ]);
  const [ttl, setTtl] = useState(3600);
  const [scopeError, setScopeError] = useState(false);
  const [configuration, setConfiguration] = useState<AgentConfig | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [consent, setConsent] = useState(false);
  const [lookupGrant, setLookupGrant] = useState("");
  const [lookupKey, setLookupKey] = useState("");
  const [lookupResult, setLookupResult] = useState<AgentLookupResult | null>(
    null,
  );
  const generation = useRef(0);
  useEffect(() => {
    const current = ++generation.current;
    focusAfterAction.current = false;
    setView(null);
    setConfiguration(null);
    setDialog(null);
    setConsent(false);
    setLookupResult(null);
    setLookupGrant("");
    setLookupKey("");
    setError(false);
    setStale(false);
    setMessage(null);
    if (!api) {
      setPending(false);
      return;
    }
    setPending(true);
    api
      .state()
      .then((snapshot) => {
        if (generation.current === current) setView(snapshot);
      })
      .catch(() => {
        if (generation.current === current) setError(true);
      })
      .finally(() => {
        if (generation.current === current) setPending(false);
      });
    return () => {
      generation.current++;
    };
  }, [api]);
  useEffect(() => {
    if (!pending && !dialog && focusAfterAction.current) {
      focusAfterAction.current = false;
      headingRef.current?.focus();
    }
  }, [pending, dialog]);
  const projectId = view?.projects.some((p) => p.id === project)
    ? project
    : (view?.projects[0]?.id ?? "");
  const ready =
    !!api && view?.owner?.state === "connected" && !pending && !stale;
  const projectName = (id: string) =>
    view?.projects.find((p) => p.id === id)?.name ?? id;
  const scopeDependencyError =
    selectedScopes.length > 0 &&
    (!selectedScopes.includes("observer") ||
      ((selectedScopes.includes("execution_test") ||
        selectedScopes.includes("sensitive_operation")) &&
        !selectedScopes.includes("project_manage")));
  const timestamp = (value: number) => {
    const date = new Date(value);
    return Number.isSafeInteger(value) && !Number.isNaN(date.getTime())
      ? new Intl.DateTimeFormat(i18n.resolvedLanguage ?? i18n.language, {
          dateStyle: "medium",
          timeStyle: "short",
        }).format(date)
      : t("agentConnections.unknown");
  };
  async function action(
    run: () => Promise<void>,
    success: "done" | "grantCreated" | null = "done",
  ) {
    if (!api || pending) return;
    const current = generation.current;
    setPending(true);
    setError(false);
    setMessage(null);
    try {
      await run();
      if (generation.current !== current) return;
      setMessage(success);
      const snapshot = await api.state();
      if (generation.current !== current) return;
      setView(snapshot);
      setStale(false);
    } catch {
      if (generation.current === current) {
        setError(true);
        setStale(true);
      }
    } finally {
      if (generation.current === current) setPending(false);
    }
  }
  const reviewComplete = (plan: AgentPlan) => {
    if (["expired", "revoked", "stale", "claimed"].includes(plan.state))
      return false;
    if (plan.target.kind === "execution") {
      const target = plan.target;
      return (
        ["php", "artisan", "composer", "node", "npm", "git"].includes(
          target.tool,
        ) &&
        Number.isSafeInteger(target.arg_count) &&
        target.arg_count >= 0 &&
        target.arg_count <= 64 &&
        Array.isArray(target.safe_arguments) &&
        target.safe_arguments.length === target.arg_count &&
        target.safe_arguments.every(
          (argument) => typeof argument === "string",
        ) &&
        Number.isSafeInteger(target.timeout_ms) &&
        target.timeout_ms > 0 &&
        target.timeout_ms <= 300000 &&
        typeof target.managed_test === "boolean" &&
        !!target.context &&
        ["environment", "database", "cache", "mail", "test_isolated"].every(
          (key) =>
            typeof target.context[key as keyof typeof target.context] ===
            "boolean",
        )
      );
    }
    if (
      !["yaml_save", "environment_apply", "domain"].includes(plan.target.kind)
    )
      return true;
    if (!plan.details?.length) return false;
    if (plan.target.kind === "domain") {
      const fingerprint = plan.details.find(
        (detail) => detail.key === "ca_fingerprint",
      );
      const remaining = plan.details.find(
        (detail) => detail.key === "remaining_ms",
      );
      const steps = plan.details.filter((detail) =>
        /^steps\.\d+\.operation$/.test(detail.key),
      );
      const completeSteps =
        steps.length > 0 &&
        steps.every((step) => {
          const target = plan.details?.find(
            (detail) => detail.key === step.key.replace(/operation$/, "target"),
          );
          if (step.value === "trust_install")
            return target?.value === "system_trust_store";
          if (step.value !== "hosts_update" || target?.value !== "system_hosts")
            return false;
          return ["hosts.added", "hosts.removed", "hosts.retained"].every(
            (key) => {
              const value = plan.details?.find(
                (detail) => detail.key === key,
              )?.value;
              return (
                Array.isArray(value) &&
                value.every((entry) => typeof entry === "string")
              );
            },
          );
        });
      return (
        completeSteps &&
        typeof fingerprint?.value === "string" &&
        fingerprint.value.length > 0 &&
        typeof remaining?.value === "number" &&
        Number.isSafeInteger(remaining.value) &&
        remaining.value > 0
      );
    }
    return true;
  };
  function openDialog(value: Dialog) {
    setConsent(false);
    setDialog(value);
  }
  async function confirm() {
    if (!api || !dialog || !ready) return;
    if (dialog.kind !== "revoke" && (!consent || !reviewComplete(dialog.plan)))
      return;
    const selected = dialog;
    const current = generation.current;
    await action(async () => {
      if (selected.kind === "revoke") {
        await api.revoke(selected.grant.grant_id);
        if (generation.current === current)
          setConfiguration((previous) =>
            previous?.grant_id === selected.grant.grant_id ? null : previous,
          );
      } else if (selected.kind === "approve")
        await api.approve(selected.plan.digest);
      else await api.confirmMachine(selected.plan.digest);
      if (generation.current === current) {
        focusAfterAction.current = true;
        setDialog(null);
      }
    });
  }
  return (
    <section
      className="card agent-connections"
      aria-labelledby={heading}
      aria-busy={pending}
    >
      <div className="section-heading">
        <Link2 aria-hidden="true" />
        <div>
          <h2 id={heading} ref={headingRef} tabIndex={-1}>
            {t("agentConnections.title")}
          </h2>
          <p>{t("agentConnections.description")}</p>
        </div>
      </div>
      {!api ? (
        <p className="notice">{t("agentConnections.unavailable")}</p>
      ) : (
        <>
          <div className="agent-toolbar">
            <p className="agent-owner">
              {t("agentConnections.owner")}
              {": "}
              <strong>
                {t(
                  view?.owner
                    ? `agentConnections.ownerState.${view.owner.state}`
                    : "agentConnections.unknown",
                )}
              </strong>
            </p>
            <button
              type="button"
              disabled={pending}
              onClick={() =>
                void action(async () => {
                  await api.connect();
                })
              }
            >
              {t("agentConnections.connect")}
            </button>
            <button
              type="button"
              disabled={pending}
              onClick={() => void action(async () => {}, null)}
            >
              {t("agentConnections.refresh")}
            </button>
          </div>
          {pending && (
            <p role="status">
              {t(
                view ? "agentConnections.pending" : "agentConnections.loading",
              )}
            </p>
          )}
          {error && (
            <p role="alert" className="notice error">
              {t("agentConnections.failed")}
            </p>
          )}
          {stale && view && (
            <p className="muted">{t("agentConnections.previous")}</p>
          )}
          {message && <p role="status">{t(`agentConnections.${message}`)}</p>}
          {view && (
            <>
              <form
                className="agent-form"
                onSubmit={(event) => {
                  event.preventDefault();
                  if (!ready || !projectId) return;
                  if (!selectedScopes.length) {
                    setScopeError(true);
                    return;
                  }
                  if (scopeDependencyError) return;
                  setScopeError(false);
                  setConfiguration(null);
                  const current = generation.current;
                  void action(async () => {
                    const issued = await api.issue({
                      project_id: projectId,
                      scopes: selectedScopes,
                      ttl_seconds: ttl,
                    });
                    if (generation.current === current)
                      setConfiguration(issued.config);
                  }, "grantCreated");
                }}
              >
                <h3>{t("agentConnections.grantTitle")}</h3>
                <fieldset disabled={!ready}>
                  <div className="agent-fields">
                    <label>
                      {t("agentConnections.project")}
                      <select
                        name="agent-project"
                        value={projectId}
                        required
                        onChange={(event) => {
                          setProject(event.target.value);
                          setConfiguration(null);
                        }}
                      >
                        <option value="">
                          {t("agentConnections.selectProject")}
                        </option>
                        {view.projects.map((p) => (
                          <option key={p.id} value={p.id}>
                            {p.name}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label>
                      {t("agentConnections.ttl")}
                      <select
                        name="agent-ttl"
                        value={ttl}
                        onChange={(event) => setTtl(Number(event.target.value))}
                      >
                        {[
                          [3600, "hour"],
                          [86400, "day"],
                          [604800, "week"],
                          [2592000, "month"],
                        ].map(([value, label]) => (
                          <option key={value} value={value}>
                            {t(`agentConnections.${label}`)}
                          </option>
                        ))}
                      </select>
                    </label>
                  </div>
                  <fieldset
                    aria-describedby={`${heading}-scope-hint${scopeError ? ` ${heading}-scope-error` : ""}${scopeDependencyError ? ` ${heading}-scope-dependency` : ""}`}
                  >
                    <legend>{t("agentConnections.scopeLegend")}</legend>
                    <p id={`${heading}-scope-hint`} className="muted">
                      {t("agentConnections.scopeHint")}
                    </p>
                    <div className="agent-scopes">
                      {scopes.map((scope) => (
                        <label className="agent-check" key={scope}>
                          <input
                            type="checkbox"
                            name={`agent-${scope}`}
                            checked={selectedScopes.includes(scope)}
                            onChange={(event) => {
                              setScopeError(false);
                              setSelectedScopes((current) =>
                                event.target.checked
                                  ? [...current, scope]
                                  : current.filter((item) => item !== scope),
                              );
                            }}
                          />
                          <span>{t(`agentConnections.scopes.${scope}`)}</span>
                        </label>
                      ))}
                    </div>
                  </fieldset>
                  {scopeDependencyError && (
                    <p id={`${heading}-scope-dependency`} role="alert">
                      {t("agentConnections.scopeDependency")}
                    </p>
                  )}
                  {scopeError && (
                    <p id={`${heading}-scope-error`} role="alert">
                      {t("agentConnections.noScopes")}
                    </p>
                  )}
                  <div className="agent-actions">
                    <button
                      className="primary"
                      type="submit"
                      disabled={!projectId || scopeDependencyError}
                    >
                      {t("agentConnections.createGrant")}
                    </button>
                  </div>
                </fieldset>
              </form>
              {configuration && (
                <div className="agent-review">
                  <h3>{t("agentConnections.configTitle")}</h3>
                  <p>{t("agentConnections.configHint")}</p>
                  <p>
                    {t("agentConnections.configPath")}
                    {": "}
                    <code translate="no">{configuration.credential_path}</code>
                  </p>
                  <p>
                    {t("agentConnections.savedConfig")}
                    {": "}
                    <code translate="no">{configuration.config_path}</code>
                  </p>
                  <label>
                    {t("agentConnections.configSnippet")}
                    <textarea
                      readOnly
                      value={configuration.config_snippet}
                      spellCheck={false}
                      translate="no"
                    />
                  </label>
                </div>
              )}
              <h3>{t("agentConnections.grants")}</h3>
              {!view.grants.length ? (
                <p className="muted">{t("agentConnections.emptyGrants")}</p>
              ) : (
                <ul>
                  {view.grants.map((grant) => (
                    <li className="agent-entry" key={grant.grant_id}>
                      <strong translate="no">
                        {projectName(grant.project_id)}
                      </strong>
                      <p>
                        {t(
                          grant.revoked
                            ? "agentConnections.revoked"
                            : grant.expired
                              ? "agentConnections.expired"
                              : "agentConnections.active",
                        )}
                        {" · "}
                        {t("agentConnections.expires")}
                        {": "}
                        {timestamp(grant.expires_at)}
                      </p>
                      <p>
                        {grant.scopes
                          .map((scope) => t(`agentConnections.scopes.${scope}`))
                          .join(" · ")}
                      </p>
                      <details>
                        <summary>{t("agentConnections.client")}</summary>
                        <code translate="no">{grant.client_id}</code>
                      </details>
                      <div className="agent-actions">
                        <button
                          type="button"
                          disabled={!ready || grant.revoked || grant.expired}
                          onClick={() => {
                            const current = generation.current;
                            setConfiguration(null);
                            void action(async () => {
                              const config = await api.config(grant.grant_id);
                              if (generation.current === current)
                                setConfiguration(config);
                            });
                          }}
                        >
                          {t("agentConnections.viewConfig")}
                        </button>
                        <button
                          type="button"
                          disabled={!ready || grant.revoked}
                          onClick={() => openDialog({ kind: "revoke", grant })}
                        >
                          {t("agentConnections.revoke")}
                        </button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
              <h3>{t("agentConnections.plans")}</h3>
              {!view.plans.length ? (
                <p className="muted">{t("agentConnections.emptyPlans")}</p>
              ) : (
                <ul>
                  {view.plans.map((plan) => (
                    <li className="agent-entry" key={plan.digest}>
                      <strong translate="no">
                        {projectName(plan.project_id)}
                      </strong>
                      <p>
                        <code translate="no">{plan.tool}</code>
                        {" · "}
                        {t("agentConnections.expires")}
                        {": "}
                        {timestamp(plan.expires_at)}
                      </p>
                      <p>{t(`agentConnections.planState.${plan.state}`)}</p>
                      <div className="agent-actions">
                        <button
                          type="button"
                          disabled={!ready || plan.state !== "pending"}
                          onClick={() => openDialog({ kind: "approve", plan })}
                        >
                          {t("agentConnections.review")}
                        </button>
                        {plan.target.kind === "domain" &&
                          plan.machine_confirmation_required && (
                            <button
                              type="button"
                              disabled={
                                !ready ||
                                !(plan.state === "approved") ||
                                [
                                  "claimed",
                                  "expired",
                                  "revoked",
                                  "stale",
                                ].includes(plan.state)
                              }
                              onClick={() =>
                                openDialog({ kind: "domain", plan })
                              }
                            >
                              {t("agentConnections.domainConfirm")}
                            </button>
                          )}
                      </div>
                    </li>
                  ))}
                </ul>
              )}
              <h3>{t("agentConnections.operations")}</h3>
              {!view.operations.length ? (
                <p className="muted">{t("agentConnections.emptyOperations")}</p>
              ) : (
                <ul>
                  {view.operations.map(({ grant_id, receipt: operation }) => (
                    <li
                      className="agent-entry"
                      key={`${grant_id}:${operation.operation_id}`}
                    >
                      <strong translate="no">
                        {projectName(operation.project_id)}
                      </strong>
                      <p>
                        {t("agentConnections.operation")}
                        {": "}
                        <code translate="no">{operation.operation_id}</code>
                        {" · "}
                        {t(
                          `agentConnections.operationState.${["queued", "running", "cancel_requested", "succeeded", "failed", "cancelled", "interrupted", "unknown"].includes(operation.state) ? operation.state : "unknown"}`,
                        )}
                      </p>
                      {["interrupted", "unknown"].includes(operation.state) && (
                        <p className="muted">
                          {t("agentConnections.outcomeUnknown")}
                        </p>
                      )}
                      <div className="agent-actions">
                        <button
                          type="button"
                          disabled={!ready}
                          onClick={() =>
                            void action(async () => {
                              await api.status({
                                grant_id,
                                operation_id: operation.operation_id,
                              });
                            })
                          }
                        >
                          {t("agentConnections.checkStatus")}
                        </button>
                        <button
                          type="button"
                          disabled={
                            !ready ||
                            !["queued", "running"].includes(operation.state)
                          }
                          onClick={() =>
                            void action(async () => {
                              await api.cancel({
                                grant_id,
                                operation_id: operation.operation_id,
                              });
                            })
                          }
                        >
                          {t("agentConnections.cancel")}
                        </button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
              <p className="muted">{t("agentConnections.cancelHint")}</p>
              <details>
                <summary>{t("agentConnections.reconcile")}</summary>
                <form
                  className="agent-form"
                  onSubmit={(event) => {
                    event.preventDefault();
                    if (!ready || !lookupGrant || !lookupKey.trim()) return;
                    setLookupResult(null);
                    const current = generation.current;
                    void action(async () => {
                      const result = await api.reconcile({
                        grant_id: lookupGrant,
                        idempotency_key: lookupKey.trim(),
                      });
                      if (generation.current === current)
                        setLookupResult(result);
                    });
                  }}
                >
                  <label>
                    {t("agentConnections.lookupGrant")}
                    <select
                      name="lookup-grant"
                      required
                      disabled={!ready}
                      value={lookupGrant}
                      onChange={(event) => {
                        setLookupGrant(event.target.value);
                        setLookupResult(null);
                      }}
                    >
                      <option value="">
                        {t("agentConnections.lookupChoose")}
                      </option>
                      {view.grants.map((grant) => (
                        <option key={grant.grant_id} value={grant.grant_id}>
                          {projectName(grant.project_id)} · {grant.grant_id}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label>
                    {t("agentConnections.lookupKey")}
                    <input
                      name="lookup-idempotency"
                      value={lookupKey}
                      maxLength={128}
                      required
                      disabled={!ready}
                      autoComplete="off"
                      spellCheck={false}
                      onChange={(event) => {
                        setLookupKey(event.target.value);
                        setLookupResult(null);
                      }}
                    />
                  </label>
                  <button
                    type="submit"
                    disabled={!ready || !lookupGrant || !lookupKey.trim()}
                  >
                    {t("agentConnections.reconcile")}
                  </button>
                  {lookupResult && (
                    <p role="status">
                      {t(
                        lookupResult.receipt
                          ? "agentConnections.lookupFound"
                          : "agentConnections.lookupMissing",
                      )}
                      {lookupResult.receipt && (
                        <>
                          {" "}
                          <code translate="no">
                            {lookupResult.receipt.operation_id}
                          </code>
                        </>
                      )}
                    </p>
                  )}
                </form>
              </details>
            </>
          )}
        </>
      )}
      {dialog && (
        <WorkspaceModal
          title={t(
            `agentConnections.${dialog.kind === "revoke" ? "revokeTitle" : dialog.kind === "domain" ? "domainTitle" : "reviewTitle"}`,
          )}
          busy={pending}
          onClose={() => setDialog(null)}
          className="agent-dialog"
        >
          <p>
            {t(
              `agentConnections.${dialog.kind === "revoke" ? "revokeHint" : dialog.kind === "domain" ? "domainHint" : "reviewHint"}`,
            )}
          </p>
          {dialog.kind === "revoke" ? (
            <p>
              <strong translate="no">
                {projectName(dialog.grant.project_id)}
              </strong>
            </p>
          ) : (
            <>
              <dl>
                <dt>{t("agentConnections.project")}</dt>
                <dd translate="no">{projectName(dialog.plan.project_id)}</dd>
                <dt>{t("agentConnections.tool")}</dt>
                <dd>
                  <code translate="no">{dialog.plan.tool}</code>
                </dd>
                <dt>{t("agentConnections.digest")}</dt>
                <dd>
                  <code translate="no">{dialog.plan.digest}</code>
                </dd>
                <dt>{t("agentConnections.expires")}</dt>
                <dd>{timestamp(dialog.plan.expires_at)}</dd>
              </dl>
              {!reviewComplete(dialog.plan) && (
                <p role="alert" className="notice error">
                  {t("agentConnections.reviewUnavailable")}
                </p>
              )}
              <PlanReview
                plan={dialog.plan}
                machine={dialog.kind === "domain"}
              />
              <label className="agent-check">
                <input
                  type="checkbox"
                  checked={consent}
                  onChange={(event) => setConsent(event.target.checked)}
                  data-initial-focus
                />
                <span>
                  {t(
                    dialog.kind === "domain"
                      ? "agentConnections.domainConsent"
                      : "agentConnections.reviewConsent",
                  )}
                </span>
              </label>
            </>
          )}
          <div className="agent-actions">
            <button
              type="button"
              disabled={pending}
              onClick={() => setDialog(null)}
              data-initial-focus={dialog.kind === "revoke" ? true : undefined}
            >
              {t("agentConnections.close")}
            </button>
            <button
              className="primary"
              type="button"
              disabled={
                !ready ||
                (dialog.kind !== "revoke" &&
                  (!consent || !reviewComplete(dialog.plan)))
              }
              onClick={() => void confirm()}
            >
              {t(
                dialog.kind === "revoke"
                  ? "agentConnections.revoke"
                  : dialog.kind === "domain"
                    ? "agentConnections.domainConfirm"
                    : "agentConnections.approve",
              )}
            </button>
          </div>
        </WorkspaceModal>
      )}
    </section>
  );
}

function PlanReview({ plan, machine }: { plan: AgentPlan; machine: boolean }) {
  const { t, i18n } = useTranslation();
  const target = plan.target;
  return (
    <>
      <dl>
        <dt>{t("agentConnections.projectRoot")}</dt>
        <dd>
          <code translate="no">{plan.project_root}</code>
        </dd>
        <dt>{t("agentConnections.revision")}</dt>
        <dd>{plan.revision}</dd>
        <dt>{t("agentConnections.lookupGrant")}</dt>
        <dd>
          <code translate="no">{plan.grant_id}</code>
        </dd>
        <dt>{t("agentConnections.client")}</dt>
        <dd>
          <code translate="no">{plan.client_id}</code>
        </dd>
        <dt>{t("agentConnections.scopeLegend")}</dt>
        <dd>
          {plan.required_scopes
            .map((scope) => t(`agentConnections.scopes.${scope}`))
            .join(" · ")}
        </dd>
      </dl>
      <h3>
        {t(
          machine
            ? "agentConnections.machineChanges"
            : "agentConnections.exactChanges",
        )}
      </h3>
      <dl>
        <dt>{t("agentConnections.targetKind")}</dt>
        <dd>{t(`agentConnections.targets.${target.kind}`)}</dd>
        {target.kind === "execution" && (
          <>
            <dt>{t("agentConnections.execCommand")}</dt>
            <dd>
              <code translate="no">{target.tool}</code>
              <ol>
                {target.safe_arguments?.map((argument, index) => (
                  <li key={index}>
                    <code translate="no">{argument}</code>
                  </li>
                ))}
              </ol>
              <p className="muted">{t("agentConnections.execSummaryHint")}</p>
            </dd>
            <dt>{t("agentConnections.execTimeout")}</dt>
            <dd>{target.timeout_ms}</dd>
            <dt>{t("agentConnections.execManagedTest")}</dt>
            <dd>
              {t(
                target.managed_test
                  ? "agentConnections.yes"
                  : "agentConnections.no",
              )}
            </dd>
            <dt>{t("agentConnections.execContext")}</dt>
            <dd>
              <ul>
                {target.context &&
                  (
                    [
                      "environment",
                      "database",
                      "cache",
                      "mail",
                      "test_isolated",
                    ] as const
                  ).map((key) => (
                    <li key={key}>
                      <code translate="no">{key}</code>
                      {": "}
                      {t(
                        typeof target.context[key] !== "boolean"
                          ? "agentConnections.unknown"
                          : target.context[key]
                            ? "agentConnections.yes"
                            : "agentConnections.no",
                      )}
                    </li>
                  ))}
              </ul>
            </dd>
          </>
        )}
        {target.kind === "service" && (
          <>
            <dt>{t("agentConnections.tool")}</dt>
            <dd>
              <code translate="no">{target.action}</code>
            </dd>
          </>
        )}
        {target.kind === "runtime_install" && (
          <>
            <dt>{t("agentConnections.package")}</dt>
            <dd>
              <code translate="no">{target.package_id}</code>
            </dd>
          </>
        )}
        {target.kind === "runtime_select" && (
          <>
            <dt>PHP</dt>
            <dd>
              <code translate="no">{target.php}</code>
            </dd>
            <dt>Node.js</dt>
            <dd>
              <code translate="no">{target.node}</code>
            </dd>
          </>
        )}
        {target.kind === "environment_apply" && (
          <>
            <dt>{t("agentConnections.writeEnv")}</dt>
            <dd>
              {t(
                target.write_env
                  ? "agentConnections.yes"
                  : "agentConnections.no",
              )}
            </dd>
          </>
        )}
        {target.kind === "yaml_save" && (
          <>
            <dt>{t("agentConnections.yamlBytes")}</dt>
            <dd>{target.byte_count}</dd>
          </>
        )}
        {target.kind === "database_bind" && (
          <>
            <dt>{t("agentConnections.databaseInstance")}</dt>
            <dd>
              <code translate="no">{target.instance_id}</code>
            </dd>
          </>
        )}
        {target.kind === "database_restore" && (
          <>
            <dt>{t("agentConnections.backup")}</dt>
            <dd>
              <code translate="no">{target.backup_id}</code>
            </dd>
          </>
        )}
        {target.kind === "domain" && (
          <>
            <dt>{t("agentConnections.tool")}</dt>
            <dd>{t(`agentConnections.domainActions.${target.action}`)}</dd>
          </>
        )}
        {plan.details?.map((detail) => {
          const key = detail.key
            .replace(/^steps\.\d+\./, "steps.")
            .replaceAll(".", "_");
          const label = `agentConnections.detailKeys.${key}`;
          return (
            <div className="agent-detail-pair" key={detail.key}>
              <dt>
                {t(label, { defaultValue: t("agentConnections.detail") })}
                {!i18n.exists(label) && (
                  <>
                    {" "}
                    <code translate="no">{detail.key}</code>
                  </>
                )}
              </dt>
              <dd>
                <code translate="no">
                  {Array.isArray(detail.value)
                    ? detail.value.join(" · ")
                    : String(detail.value)}
                </code>
              </dd>
            </div>
          );
        })}
      </dl>
      {plan.revalidation_required && (
        <p className="muted">{t("agentConnections.revalidateHint")}</p>
      )}
      <details>
        <summary>{t("agentConnections.effects")}</summary>
        <ul>
          {plan.effects.map((effect) => (
            <li key={effect}>
              <code translate="no">{effect}</code>
            </li>
          ))}
        </ul>
      </details>
    </>
  );
}
