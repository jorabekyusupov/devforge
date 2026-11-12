import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderGit2, FolderPlus, CheckCircle2 } from "lucide-react";
import { WorkspaceModal } from "./WorkspaceModal";
import type {
  Command,
  CreateInput,
  Failure,
  HttpsReview,
  PhpSettings,
  PrepareOptions,
  Row,
  Runtime,
  SetupProgress,
} from "../workspace";
type Run = (
  c: Command,
  onResult?: (v: Record<string, unknown>) => void,
) => Promise<boolean>;
function useAlertFocus(message: string) {
  const target = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (message) target.current?.focus();
  }, [message]);
  return (node: HTMLElement | null) => {
    target.current = node;
  };
}
const defaults = (sqlite = false): PrepareOptions => ({
  composer: true,
  npm: false,
  create_env: true,
  generate_key: true,
  run_scripts: false,
  initialize_sqlite: sqlite,
  trusted: false,
});
const slug = (s: string) =>
  s
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/^-|-$/g, "");
const pathLabel = (parent: string, folder: string) =>
  `${parent}${/[\\/]$/.test(parent) ? "" : parent.includes("\\") ? "\\" : "/"}${folder}`;
function RuntimeField({
  kind,
  list,
  value,
  onChange,
}: {
  kind: "php" | "node";
  list: Runtime[];
  value: string;
  onChange: (v: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <label>
      {t(`work.kinds.${kind}`)}
      <select
        name={`setup-${kind}`}
        value={value}
        required
        onChange={(e) => onChange(e.target.value)}
      >
        <option value="">{t("setup.chooseRuntime")}</option>
        {list
          .filter(
            (r) =>
              r.kind === kind &&
              (kind !== "php" ||
                r.variant === "cli" ||
                r.id.includes("windows")),
          )
          .map((r) => (
            <option key={r.id} value={r.id} disabled={!r.installed}>
              {r.version}
              {r.installed ? "" : ` · ${t("work.states.missing")}`}
            </option>
          ))}
      </select>
    </label>
  );
}
export function PreparationOptions({
  value,
  onChange,
  sqlite = false,
}: {
  value: PrepareOptions;
  onChange: (v: PrepareOptions) => void;
  sqlite?: boolean;
}) {
  const { t } = useTranslation();
  const keys = (
    sqlite
      ? [
          "composer",
          "npm",
          "create_env",
          "generate_key",
          "initialize_sqlite",
          "run_scripts",
        ]
      : ["composer", "npm", "create_env", "generate_key", "run_scripts"]
  ) as (keyof PrepareOptions)[];
  return (
    <div className="setup-options">
      {keys.map((key) => (
        <label key={key} className="setup-check">
          <input
            type="checkbox"
            name={key}
            checked={value[key]}
            onChange={(e) => onChange({ ...value, [key]: e.target.checked })}
          />
          <span>{t(`setup.options.${key}`)}</span>
        </label>
      ))}
      <p className="muted">{t("setup.preserveKeys")}</p>
    </div>
  );
}
function Trust({
  value,
  onChange,
}: {
  value: boolean;
  onChange: (v: boolean) => void;
}) {
  const { t } = useTranslation();
  return (
    <label className="setup-check setup-consent">
      <input
        name="trusted"
        type="checkbox"
        required
        checked={value}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span>{t("setup.trust")}</span>
    </label>
  );
}
function Progress({
  value,
  cancel,
}: {
  value: SetupProgress | null;
  cancel: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="setup-progress" role="status">
      <span className="spinner" aria-hidden="true" />
      {t(value ? `setup.stages.${value.stage}` : "work.working")}
      <button type="button" onClick={cancel}>
        {t("setup.cancelOperation")}
      </button>
    </div>
  );
}
export function CreateProjectDialog({
  runtimes,
  starter,
  busy,
  error,
  operation,
  browse,
  run,
  cancel,
  onClose,
  onCreated,
}: {
  runtimes: Runtime[];
  starter: string;
  busy: boolean;
  error: Failure | null;
  operation: SetupProgress | null;
  browse: () => Promise<string | null>;
  run: Run;
  cancel: () => void;
  onClose: () => void;
  onCreated: (id: string) => void;
}) {
  const { t, i18n } = useTranslation();
  const [step, setStep] = useState(0);
  const formRef = useRef<HTMLFormElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const first = (kind: string) =>
    [...runtimes]
      .filter(
        (r) =>
          r.installed &&
          r.kind === kind &&
          (kind !== "php" || r.variant === "cli" || r.id.includes("windows")),
      )
      .sort((a, b) =>
        b.version.localeCompare(a.version, undefined, { numeric: true }),
      )[0]?.id ?? "";
  const [input, setInput] = useState<CreateInput>({
    source: { kind: "laravel", version: starter },
    parent: "",
    folder: "",
    name: "",
    php: first("php"),
    node: first("node"),
    domain: "",
    https: true,
    options: defaults(true),
  });
  const update = <K extends keyof CreateInput>(key: K, v: CreateInput[K]) =>
    setInput((p) => ({ ...p, [key]: v }));
  useEffect(() => {
    if (step) heading.current?.focus();
  }, [step]);
  const errorKey = error ? `${error.scope}.errors.${error.code}` : "";
  const errorText = error
    ? i18n.exists(errorKey)
      ? t(errorKey)
      : t("setup.failed")
    : "";
  const alertRef = useAlertFocus(errorText);
  const next = () => {
    if (formRef.current?.reportValidity()) setStep((s) => s + 1);
  };
  const finish = async () => {
    if (!formRef.current?.reportValidity()) return;
    const command: Command = error?.project_id
      ? {
          action: "prepare_project",
          id: error.project_id,
          options: input.options,
        }
      : { action: "create_project", input };
    await run(command, (result) => {
      const id = String(result.project_id ?? error?.project_id ?? "");
      if (id) onCreated(id);
    });
  };
  return (
    <WorkspaceModal
      title={t("setup.newProject")}
      className="setup-dialog"
      busy={busy}
      onClose={onClose}
    >
      <ol className="setup-steps" aria-label={t("setup.steps")}>
        {["source", "environment", "review"].map((key, n) => (
          <li
            key={key}
            aria-current={n === step ? "step" : undefined}
            className={n === step ? "active" : n < step ? "done" : ""}
          >
            <span>{n + 1}</span>
            {t(`setup.${key}`)}
          </li>
        ))}
      </ol>
      {errorText && (
        <div role="alert" tabIndex={-1} ref={alertRef} className="notice error">
          <p>{errorText}</p>
          {error?.project_id && (
            <>
              <p>{t("setup.recover")}</p>
              <button
                type="button"
                onClick={() => onCreated(error.project_id!)}
              >
                {t("setup.openRegistered")}
              </button>
            </>
          )}
        </div>
      )}
      <form
        ref={formRef}
        onSubmit={(e) => {
          e.preventDefault();
          if (step < 2) next();
          else void finish();
        }}
        onInvalidCapture={(e) =>
          (e.target as HTMLInputElement).setCustomValidity(
            t("work.invalidForm"),
          )
        }
        onInputCapture={(e) =>
          (e.target as HTMLInputElement).setCustomValidity("")
        }
      >
        <fieldset disabled={busy}>
          <h3 ref={heading} tabIndex={-1} className="setup-step-heading">
            {t(`setup.${["source", "environment", "review"][step]}`)}
          </h3>
          {step === 0 && (
            <>
              <div
                className="setup-sources"
                role="radiogroup"
                aria-label={t("setup.source")}
              >
                {(["laravel", "git"] as const).map((kind) => (
                  <label
                    key={kind}
                    className={input.source.kind === kind ? "selected" : ""}
                  >
                    <input
                      type="radio"
                      name="setup-source"
                      value={kind}
                      checked={input.source.kind === kind}
                      onChange={() => {
                        update(
                          "source",
                          kind === "laravel"
                            ? { kind, version: starter }
                            : { kind, repository: "", branch: null },
                        );
                        update("options", {
                          ...input.options,
                          initialize_sqlite: kind === "laravel",
                        });
                      }}
                    />
                    {kind === "laravel" ? (
                      <FolderPlus aria-hidden="true" />
                    ) : (
                      <FolderGit2 aria-hidden="true" />
                    )}
                    <strong>{t(`setup.sources.${kind}`)}</strong>
                  </label>
                ))}
              </div>
              {input.source.kind === "laravel" ? (
                <label>
                  {t("setup.version")}
                  <input
                    name="laravel-version"
                    value={input.source.version}
                    required
                    maxLength={80}
                    autoComplete="off"
                    spellCheck={false}
                    onChange={(e) =>
                      update("source", {
                        kind: "laravel",
                        version: e.target.value,
                      })
                    }
                  />
                  <small>{t("setup.versionHint")}</small>
                </label>
              ) : (
                <>
                  <label>
                    {t("setup.repository")}
                    <input
                      name="repository"
                      required
                      value={input.source.repository}
                      autoComplete="off"
                      spellCheck={false}
                      onChange={(e) =>
                        update("source", {
                          kind: "git",
                          repository: e.target.value,
                          branch:
                            input.source.kind === "git"
                              ? input.source.branch
                              : null,
                        })
                      }
                    />
                    <small>{t("setup.gitHint")}</small>
                  </label>
                  <label>
                    {t("setup.branch")}
                    <input
                      name="branch"
                      value={input.source.branch ?? ""}
                      autoComplete="off"
                      spellCheck={false}
                      onChange={(e) =>
                        update("source", {
                          kind: "git",
                          repository:
                            input.source.kind === "git"
                              ? input.source.repository
                              : "",
                          branch: e.target.value || null,
                        })
                      }
                    />
                  </label>
                </>
              )}
            </>
          )}
          {step === 1 && (
            <>
              <label>
                {t("work.projectName")}
                <input
                  name="setup-name"
                  value={input.name}
                  required
                  maxLength={120}
                  onChange={(e) =>
                    setInput((old) => ({
                      ...old,
                      name: e.target.value,
                      folder:
                        !old.folder || old.folder === slug(old.name)
                          ? slug(e.target.value)
                          : old.folder,
                      domain:
                        !old.domain ||
                        old.domain === `${slug(old.name)}.devforge.test`
                          ? `${slug(e.target.value)}.devforge.test`
                          : old.domain,
                    }))
                  }
                />
              </label>
              <div className="setup-parent-field">
                <label htmlFor="setup-parent-input">{t("setup.parent")}</label>
                <div className="input-action">
                  <input
                    id="setup-parent-input"
                    name="setup-parent"
                    value={input.parent}
                    required
                    autoComplete="off"
                    spellCheck={false}
                    onChange={(e) => update("parent", e.target.value)}
                  />
                  <button
                    type="button"
                    onClick={async () => {
                      const path = await browse();
                      if (path) update("parent", path);
                    }}
                  >
                    {t("work.browse")}
                  </button>
                </div>
              </div>
              <label>
                {t("setup.folder")}
                <input
                  name="setup-folder"
                  value={input.folder}
                  required
                  maxLength={80}
                  pattern="[a-zA-Z0-9][a-zA-Z0-9._\-]*"
                  autoComplete="off"
                  spellCheck={false}
                  onChange={(e) => update("folder", e.target.value)}
                />
              </label>
              <div className="detail-grid">
                <RuntimeField
                  kind="php"
                  list={runtimes}
                  value={input.php}
                  onChange={(v) => update("php", v)}
                />
                <RuntimeField
                  kind="node"
                  list={runtimes}
                  value={input.node}
                  onChange={(v) => update("node", v)}
                />
              </div>
              {(!input.php || !input.node) && (
                <a href="#runtimes">{t("work.checkRuntimes")}</a>
              )}
              <div className="setup-domain">
                <label>
                  {t("work.domain")}
                  <input
                    name="setup-domain"
                    value={input.domain}
                    required
                    pattern="[a-z0-9][a-z0-9.\-]*\.devforge\.test"
                    autoComplete="off"
                    spellCheck={false}
                    onChange={(e) => update("domain", e.target.value)}
                  />
                </label>
                <label className="setup-check">
                  <input
                    type="checkbox"
                    name="setup-https"
                    checked={input.https}
                    onChange={(e) => update("https", e.target.checked)}
                  />
                  HTTPS
                </label>
              </div>
              <h3>{t("setup.prepare")}</h3>
              <PreparationOptions
                value={input.options}
                onChange={(v) =>
                  update(
                    "options",
                    !v.composer ? { ...v, initialize_sqlite: false } : v,
                  )
                }
                sqlite={input.source.kind === "laravel"}
              />
            </>
          )}
          {step === 2 && (
            <>
              <dl className="setup-review">
                <div>
                  <dt>{t("work.projectName")}</dt>
                  <dd translate="no">{input.name}</dd>
                </div>
                <div>
                  <dt>{t("setup.source")}</dt>
                  <dd translate="no">
                    {input.source.kind === "laravel"
                      ? `laravel/laravel ${input.source.version}`
                      : `${input.source.repository} ${input.source.branch ?? ""}`}
                  </dd>
                </div>
                <div>
                  <dt>{t("work.path")}</dt>
                  <dd translate="no">
                    {pathLabel(input.parent, input.folder)}
                  </dd>
                </div>
                <div>
                  <dt>{t("work.domain")}</dt>
                  <dd translate="no">
                    {input.domain} · {input.https ? "HTTPS" : "HTTP"}
                  </dd>
                </div>
                <div>
                  <dt>PHP / Node.js</dt>
                  <dd translate="no">
                    {runtimes.find((r) => r.id === input.php)?.version} /{" "}
                    {runtimes.find((r) => r.id === input.node)?.version}
                  </dd>
                </div>
              </dl>
              <ul className="setup-action-list">
                {Object.entries(input.options)
                  .filter(([key, value]) => key !== "trusted" && value)
                  .map(([key]) => (
                    <li key={key}>{t(`setup.options.${key}`)}</li>
                  ))}
              </ul>
              <p className="muted">{t("setup.reviewHint")}</p>
              <Trust
                value={input.options.trusted}
                onChange={(v) =>
                  update("options", { ...input.options, trusted: v })
                }
              />
            </>
          )}
        </fieldset>
        {busy && <Progress value={operation} cancel={cancel} />}
        <div className="modal-actions">
          <button
            type="button"
            disabled={busy || !!error?.project_id}
            onClick={() => (step ? setStep((s) => s - 1) : onClose())}
          >
            {t(step ? "setup.back" : "work.cancel")}
          </button>
          <button
            className="primary"
            disabled={busy || (step === 2 && !input.options.trusted)}
            type="submit"
          >
            {t(
              step < 2
                ? "setup.next"
                : error?.project_id
                  ? "setup.retrySetup"
                  : "setup.create",
            )}
          </button>
        </div>
      </form>
    </WorkspaceModal>
  );
}
export function PrepareProjectDialog({
  row,
  busy,
  errorText,
  operation,
  run,
  cancel,
  onClose,
}: {
  row: Row;
  busy: boolean;
  errorText: string;
  operation: SetupProgress | null;
  run: Run;
  cancel: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const alertRef = useAlertFocus(errorText);
  const [options, setOptions] = useState(defaults());
  return (
    <WorkspaceModal
      title={`${t("setup.prepare")} · ${row.project.name}`}
      className="setup-dialog compact"
      busy={busy}
      onClose={onClose}
    >
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (
            await run({
              action: "prepare_project",
              id: row.project.id,
              options,
            })
          )
            onClose();
        }}
      >
        {errorText && (
          <p role="alert" tabIndex={-1} ref={alertRef} className="modal-error">
            {errorText}
          </p>
        )}
        <fieldset disabled={busy}>
          <p>{t("setup.setupChanges")}</p>
          {row.state !== "stopped" && (
            <p className="notice">{t("setup.stopFirst")}</p>
          )}
          <PreparationOptions
            value={options}
            onChange={setOptions}
            sqlite={row.setup_owned}
          />
          <Trust
            value={options.trusted}
            onChange={(v) => setOptions((p) => ({ ...p, trusted: v }))}
          />
        </fieldset>
        {busy && <Progress value={operation} cancel={cancel} />}
        <div className="modal-actions">
          <button disabled={busy} type="button" onClick={onClose}>
            {t("work.cancel")}
          </button>
          <button
            className="primary"
            disabled={busy || !options.trusted || row.state !== "stopped"}
          >
            {t("setup.runSetup")}
          </button>
        </div>
      </form>
    </WorkspaceModal>
  );
}
export function PhpSettingsDialog({
  row,
  busy,
  errorText,
  run,
  onClose,
}: {
  row: Row;
  busy: boolean;
  errorText: string;
  run: Run;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const alertRef = useAlertFocus(errorText);
  const [openedRevision] = useState(row.project.revision);
  const [settings, setSettings] = useState<PhpSettings>(
    row.project.php_settings ?? {
      memory_limit: null,
      upload_mb: null,
      post_mb: null,
      execution_seconds: null,
      timezone: null,
    },
  );
  const update = <K extends keyof PhpSettings>(key: K, value: PhpSettings[K]) =>
    setSettings((p) => ({ ...p, [key]: value }));
  const d = row.php_diagnostics;
  return (
    <WorkspaceModal
      title={`${t("setup.phpSettings")} · ${row.project.name}`}
      className="setup-dialog compact"
      busy={busy}
      onClose={onClose}
    >
      {errorText && (
        <p className="modal-error" role="alert" tabIndex={-1} ref={alertRef}>
          {errorText}
        </p>
      )}
      <p className="muted">{t("setup.phpScope")}</p>
      <button
        type="button"
        disabled={busy}
        onClick={() =>
          void run({ action: "php_diagnostics", id: row.project.id })
        }
      >
        {t("setup.phpProbe")}
      </button>
      {d && (
        <section className="php-diagnostics" aria-label={t("setup.phpProbe")}>
          <dl>
            <div>
              <dt>PHP</dt>
              <dd translate="no">
                {d.version} · {d.sapi}
              </dd>
            </div>
            <div>
              <dt>{t("setup.loadedIni")}</dt>
              <dd translate="no">{d.ini}</dd>
            </div>
            <div>
              <dt>{t("setup.vendor")}</dt>
              <dd>{t(d.vendor_present ? "setup.present" : "setup.missing")}</dd>
            </div>
            <div>
              <dt>.env / APP_KEY</dt>
              <dd>
                {t(
                  d.environment_exists && d.app_key_present
                    ? "setup.present"
                    : "setup.missing",
                )}
              </dd>
            </div>
          </dl>
          <h3>{t("setup.extensions")}</h3>
          <p className="extension-list" translate="no">
            {d.extensions.join(", ")}
          </p>
          <h3>{t("setup.platformRequirements")}</h3>
          {d.platform_check_available ? (
            <ul className="platform-requirements">
              {d.platform_requirements.map((r) => (
                <li key={r.name}>
                  <code translate="no">
                    {r.name} {r.version}
                  </code>
                  <span
                    className={r.status === "success" ? "" : "platform-missing"}
                  >
                    {t(
                      r.status === "success"
                        ? "setup.available"
                        : "setup.unmet",
                    )}
                  </span>
                </li>
              ))}
            </ul>
          ) : (
            <p>{t("setup.platformUnavailable")}</p>
          )}
          <details>
            <summary>{t("setup.effectiveSettings")}</summary>
            <pre tabIndex={0}>{JSON.stringify(d.settings, null, 2)}</pre>
          </details>
        </section>
      )}
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (
            await run({
              action: "php_settings",
              id: row.project.id,
              revision: openedRevision,
              settings,
            })
          )
            onClose();
        }}
        onInvalidCapture={(e) =>
          (e.target as HTMLInputElement).setCustomValidity(
            t("work.invalidForm"),
          )
        }
        onInputCapture={(e) =>
          (e.target as HTMLInputElement).setCustomValidity("")
        }
      >
        <fieldset disabled={busy}>
          <div className="detail-grid">
            <label>
              {t("setup.memory")}
              <input
                name="memory-limit"
                placeholder={t("setup.runtimeDefault")}
                value={settings.memory_limit ?? ""}
                pattern="-1|[0-9]+M"
                onChange={(e) => update("memory_limit", e.target.value || null)}
              />
            </label>
            <label>
              {t("setup.execution")}
              <input
                name="execution-seconds"
                type="number"
                min={0}
                max={3600}
                placeholder={t("setup.runtimeDefault")}
                value={settings.execution_seconds ?? ""}
                onChange={(e) =>
                  update(
                    "execution_seconds",
                    e.target.value === "" ? null : Number(e.target.value),
                  )
                }
              />
            </label>
            <label>
              {t("setup.upload")}
              <input
                name="upload-mb"
                required={settings.post_mb !== null}
                type="number"
                min={1}
                max={4096}
                placeholder={t("setup.runtimeDefault")}
                value={settings.upload_mb ?? ""}
                onChange={(e) =>
                  update(
                    "upload_mb",
                    e.target.value === "" ? null : Number(e.target.value),
                  )
                }
              />
            </label>
            <label>
              {t("setup.post")}
              <input
                name="post-mb"
                required={settings.upload_mb !== null}
                type="number"
                min={settings.upload_mb ?? 1}
                max={4096}
                placeholder={t("setup.runtimeDefault")}
                value={settings.post_mb ?? ""}
                onChange={(e) =>
                  update(
                    "post_mb",
                    e.target.value === "" ? null : Number(e.target.value),
                  )
                }
              />
            </label>
          </div>
          <label>
            {t("setup.timezone")}
            <input
              name="php-timezone"
              placeholder="UTC"
              value={settings.timezone ?? ""}
              autoComplete="off"
              spellCheck={false}
              onChange={(e) => update("timezone", e.target.value || null)}
            />
          </label>
        </fieldset>
        <div className="modal-actions">
          <button type="button" disabled={busy} onClick={onClose}>
            {t("work.cancel")}
          </button>
          <button className="primary" disabled={busy}>
            {t("work.save")}
          </button>
        </div>
      </form>
    </WorkspaceModal>
  );
}
export function HttpsSetupDialog({
  row,
  review,
  busy,
  errorText,
  run,
  onClose,
}: {
  row: Row;
  review: HttpsReview | null;
  busy: boolean;
  errorText: string;
  run: Run;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const alertRef = useAlertFocus(errorText);
  const [consent, setConsent] = useState(false);
  const p = review?.preview;
  const ready = !!p?.system_trusted && !!p?.hosts_ready;
  return (
    <WorkspaceModal
      title={`${t("setup.systemHttps")} · ${row.project.name}`}
      className="setup-dialog compact"
      busy={busy}
      onClose={onClose}
    >
      {errorText && (
        <p className="modal-error" role="alert" tabIndex={-1} ref={alertRef}>
          {errorText}
        </p>
      )}
      {!p ? (
        <p role="status">{t("work.loading")}</p>
      ) : (
        <>
          <p role="status" className="notice">
            {ready ? <CheckCircle2 aria-hidden="true" /> : null}
            {t(ready ? "setup.systemReady" : "setup.systemPending")}
          </p>
          <dl className="setup-review">
            <div>
              <dt>{t("setup.hostsPath")}</dt>
              <dd translate="no">{p.hosts_path}</dd>
            </div>
            <div>
              <dt>{t("setup.domainChanges")}</dt>
              <dd translate="no">{p.domains.join(", ")}</dd>
            </div>
            <div>
              <dt>{t("setup.fingerprint")}</dt>
              <dd className="certificate-fingerprint" translate="no">
                {p.fingerprint}
              </dd>
            </div>
          </dl>
          <p>{t("setup.authorization")}</p>
          {!p.https_enabled && (
            <p className="notice">{t("setup.enableHttps")}</p>
          )}
          {!p.helper_available && (
            <p className="notice">{t("setup.helperMissing")}</p>
          )}
          <label className="setup-check setup-consent">
            <input
              type="checkbox"
              checked={consent}
              disabled={busy || ready}
              onChange={(e) => setConsent(e.target.checked)}
            />
            <span>{t("setup.systemConsent")}</span>
          </label>
        </>
      )}
      <div className="modal-actions">
        <button disabled={busy} onClick={onClose}>
          {t("work.close")}
        </button>
        <button
          disabled={busy}
          onClick={() => {
            setConsent(false);
            void run({ action: "https_export", id: row.project.id });
          }}
        >
          {t("setup.refreshReview")}
        </button>
        <button
          className="primary"
          disabled={
            busy ||
            !consent ||
            !review ||
            !p?.helper_available ||
            !p?.https_enabled ||
            ready
          }
          onClick={() => {
            if (review)
              void run({
                action: "https_apply",
                id: row.project.id,
                ticket: review.ticket,
                digest: review.digest,
                consent,
              });
          }}
        >
          {t("setup.applySystem")}
        </button>
      </div>
    </WorkspaceModal>
  );
}
