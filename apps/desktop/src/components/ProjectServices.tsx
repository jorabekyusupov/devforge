import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { WorkspaceModal } from "./WorkspaceModal";
import type {
  Snapshot,
  Row,
  Command,
  ServiceSelection,
  WorkspaceBackend,
} from "../workspace";
export function ProjectServices({
  snapshot,
  busy,
  run,
  backend,
}: {
  snapshot: Snapshot | null;
  busy: boolean;
  run: (c: Command) => Promise<boolean>;
  backend?: WorkspaceBackend;
}) {
  const { t } = useTranslation();
  const [id, setId] = useState("");
  const [dirty, setDirty] = useState(false);
  const [leave, setLeave] = useState<{
    project?: string;
    route?: string;
  } | null>(null);
  useEffect(() => {
    if (!dirty || busy) return;
    const guard = (event: MouseEvent) => {
      const link = (event.target as Element).closest<HTMLAnchorElement>(
        'a[href^="#"]',
      );
      if (
        link &&
        link.hash !== location.hash &&
        !event.metaKey &&
        !event.ctrlKey
      ) {
        event.preventDefault();
        event.stopPropagation();
        setLeave({ route: link.hash });
      }
    };
    const unload = (event: BeforeUnloadEvent) => event.preventDefault();
    document.addEventListener("click", guard, true);
    window.addEventListener("beforeunload", unload);
    return () => {
      document.removeEventListener("click", guard, true);
      window.removeEventListener("beforeunload", unload);
    };
  }, [dirty, busy]);
  const rows = snapshot?.projects ?? [];
  const row = rows.find((r) => r.project.id === id) ?? rows[0];
  return (
    <section className="work-panel service-project-panel">
      <div className="section-toolbar">
        <h2>{t("services.project")}</h2>
        <label>
          {t("services.projectLabel")}
          <select
            name="service-project"
            value={row?.project.id ?? ""}
            onChange={(e) =>
              dirty
                ? setLeave({ project: e.target.value })
                : setId(e.target.value)
            }
            disabled={busy}
          >
            {rows.map((r) => (
              <option key={r.project.id} value={r.project.id}>
                {r.project.name}
              </option>
            ))}
          </select>
        </label>
      </div>
      {row && snapshot ? (
        <ServiceForm
          key={`${row.project.id}-${row.project.revision}`}
          row={row}
          snapshot={snapshot}
          busy={busy}
          run={run}
          backend={backend}
          onDirty={setDirty}
        />
      ) : (
        <p className="empty-copy">{t("work.noProjects")}</p>
      )}
      {leave && (
        <WorkspaceModal
          title={t("work.unsaved")}
          busy={false}
          onClose={() => setLeave(null)}
        >
          <p>{t("work.unsavedHint")}</p>
          <div className="modal-actions">
            <button data-initial-focus onClick={() => setLeave(null)}>
              {t("work.keepEditing")}
            </button>
            <button
              className="danger"
              onClick={() => {
                if (leave.project) setId(leave.project);
                if (leave.route) location.hash = leave.route;
                setDirty(false);
                setLeave(null);
              }}
            >
              {t("work.discard")}
            </button>
          </div>
        </WorkspaceModal>
      )}
    </section>
  );
}
function ServiceForm({
  onDirty,
  row,
  snapshot,
  busy,
  run,
  backend,
}: {
  onDirty: (dirty: boolean) => void;
  row: Row;
  snapshot: Snapshot;
  busy: boolean;
  run: (c: Command) => Promise<boolean>;
  backend?: WorkspaceBackend;
}) {
  const { t, i18n } = useTranslation();
  const p = row.project;
  const [selection, setSelection] = useState<ServiceSelection>(
    p.services ?? { cache: "project", queue: "project", redis_instance: null },
  );
  const changed =
    JSON.stringify(selection) !==
    JSON.stringify(
      p.services ?? {
        cache: "project",
        queue: "project",
        redis_instance: null,
      },
    );
  useEffect(() => onDirty(changed), [changed, onDirty]);
  const [database, setDatabase] = useState(row.database?.instance_id ?? "");
  const garnet = snapshot.runtimes.filter((r) => r.kind === "garnet");
  const [version, setVersion] = useState(garnet[0]?.id ?? "");
  const [confirm, setConfirm] = useState<Command | null>(null);
  const [dialogError, setDialogError] = useState(false);
  const [fileError, setFileError] = useState(false);
  const active = row.state !== "stopped";
  const caches = (snapshot.caches ?? []).filter(
    (c) => c.instance.project_id === p.id,
  );
  const usesRedis = selection.cache === "redis" || selection.queue === "redis";
  const show = (command: Command) => {
    setDialogError(false);
    setConfirm(command);
  };
  async function archive(save: boolean, backup?: string) {
    setFileError(false);
    try {
      const path = await backend?.chooseBackup?.(save);
      if (path)
        await run(
          save
            ? { action: "export_backup", id: p.id, backup: backup!, path }
            : { action: "import_backup", id: p.id, path },
        );
    } catch {
      setFileError(true);
    }
  }

  return (
    <>
      {fileError && <p role="alert">{t("services.fileError")}</p>}
      <div className="service-config-grid">
        <label>
          {t("work.databaseService")}
          <select
            name="service-database"
            value={database}
            disabled={busy || active}
            onChange={(e) => setDatabase(e.target.value)}
          >
            <option value="">{t("services.drivers.project")}</option>
            {snapshot.databases
              .filter((d) => d.instance.ready)
              .map((d) => (
                <option key={d.instance.id} value={d.instance.id}>
                  {d.instance.engine === "mysql" ? "MySQL" : "PostgreSQL"} ·{" "}
                  {
                    snapshot.runtimes.find((r) => r.id === d.instance.package)
                      ?.version
                  }{" "}
                  · {d.instance.port}
                </option>
              ))}
          </select>
        </label>
        <button
          disabled={
            busy || active || database === (row.database?.instance_id ?? "")
          }
          onClick={() =>
            show(
              database
                ? {
                    action: "switch_database",
                    id: p.id,
                    instance: database,
                    consent: true,
                  }
                : { action: "unbind_database", id: p.id, consent: true },
            )
          }
        >
          {t("services.switch")}
        </button>
        <label>
          {t("services.cache")}
          <select
            disabled={busy || active}
            name="service-cache"
            value={selection.cache}
            onChange={(e) =>
              setSelection({
                ...selection,
                cache: e.target.value as ServiceSelection["cache"],
              })
            }
          >
            {["project", "file", "database", "redis"].map((v) => (
              <option key={v} value={v}>
                {t(`services.drivers.${v}`)}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t("services.queue")}
          <select
            disabled={busy || active}
            name="service-queue"
            value={selection.queue}
            onChange={(e) =>
              setSelection({
                ...selection,
                queue: e.target.value as ServiceSelection["queue"],
              })
            }
          >
            {["project", "sync", "database", "redis"].map((v) => (
              <option key={v} value={v}>
                {t(`services.drivers.${v}`)}
              </option>
            ))}
          </select>
        </label>
        <button
          className="primary"
          disabled={busy || active || (usesRedis && !selection.redis_instance)}
          onClick={() =>
            void run({
              action: "configure_services",
              id: p.id,
              revision: p.revision,
              selection,
            })
          }
        >
          {t("work.save")}
        </button>
      </div>
      <p className="muted">{t("services.selectionHint")}</p>
      <details open={usesRedis}>
        <summary>{t("services.redis")}</summary>
        <p className="muted">{t("services.redisHint")}</p>
        <div className="database-controls">
          <label>
            {t("services.cacheInstance")}
            <select
              disabled={busy || active}
              name="service-cache-instance"
              value={selection.redis_instance ?? ""}
              onChange={(e) =>
                setSelection({
                  ...selection,
                  redis_instance: e.target.value || null,
                })
              }
            >
              <option value="">{t("services.choose")}</option>
              {caches
                .filter((c) => c.instance.ready)
                .map((c) => (
                  <option key={c.instance.id} value={c.instance.id}>
                    Garnet ·{" "}
                    {
                      snapshot.runtimes.find((r) => r.id === c.instance.package)
                        ?.version
                    }{" "}
                    · {c.instance.port}
                  </option>
                ))}
            </select>
          </label>
          <label>
            {t("work.version")}
            <select
              disabled={busy}
              value={version}
              onChange={(e) => setVersion(e.target.value)}
            >
              {garnet.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.version}
                  {r.installed ? "" : ` · ${t("work.states.missing")}`}
                </option>
              ))}
            </select>
          </label>
          <button
            disabled={
              busy || active || !garnet.find((r) => r.id === version)?.installed
            }
            onClick={() =>
              void run({ action: "create_cache", id: p.id, package: version })
            }
          >
            {t("services.createCache")}
          </button>
        </div>
        <p className="muted">{t("services.predisHint")}</p>
        <button
          disabled={busy || active}
          onClick={() =>
            show({ action: "install_redis_client", id: p.id, consent: true })
          }
        >
          {t("services.installClient")}
        </button>
        {caches.map((c) => (
          <div className="service-row" key={c.instance.id}>
            <div>
              <strong>
                Garnet ·{" "}
                {
                  snapshot.runtimes.find((r) => r.id === c.instance.package)
                    ?.version
                }
              </strong>
              <p>
                <code>127.0.0.1:{c.instance.port}</code> ·{" "}
                {t(`work.states.${c.state}`)}
              </p>
            </div>
            <button
              disabled={busy || active || !c.instance.ready}
              onClick={() =>
                void run({
                  action: c.state === "stopped" ? "start_cache" : "stop_cache",
                  instance: c.instance.id,
                })
              }
            >
              {t(c.state === "stopped" ? "work.start" : "work.stop")}
            </button>
          </div>
        ))}
      </details>
      <details className="retained-databases">
        <summary>{t("services.retained")}</summary>
        <p className="muted">{t("services.retainedHint")}</p>
        {(row.database_history ?? [])
          .filter(
            (b) =>
              b.ready &&
              (b.database !== row.database?.database ||
                b.instance_id !== row.database?.instance_id),
          )
          .map((b) => (
            <div className="service-row" key={`${b.instance_id}-${b.database}`}>
              <code className="backup-id" translate="no">
                {b.database}
              </code>
              <button
                disabled={busy || active}
                onClick={() =>
                  show({
                    action: "activate_database_generation",
                    id: p.id,
                    instance: b.instance_id,
                    database: b.database,
                    consent: true,
                  })
                }
              >
                {t("services.activate")}
              </button>
            </div>
          ))}
      </details>
      <section className="backup-section">
        <div className="section-toolbar">
          <h3>{t("services.backups")}</h3>
          <button
            disabled={busy || active || !row.database}
            onClick={() => void run({ action: "create_backup", id: p.id })}
          >
            {t("services.createBackup")}
          </button>
          <button
            disabled={busy || !backend?.chooseBackup}
            onClick={() => void archive(false)}
          >
            {t("services.import")}
          </button>
        </div>
        <p className="muted">{t("services.restoreSummary")}</p>
        <details>
          <summary>{t("services.backupLimits")}</summary>
          <p className="muted">{t("services.backupHint")}</p>
        </details>
        {!(row.backups ?? []).length && (
          <p className="empty-copy">{t("services.noBackups")}</p>
        )}
        {(row.backups ?? []).map((b) => (
          <div className="service-row" key={b.id}>
            <div>
              <strong>
                {b.engine === "mysql" ? "MySQL" : "PostgreSQL"} · {b.version}
              </strong>
              <p>
                {new Intl.DateTimeFormat(i18n.language, {
                  dateStyle: "medium",
                  timeStyle: "short",
                }).format(new Date(b.created * 1000))}{" "}
                ·{" "}
                {new Intl.NumberFormat(i18n.language, {
                  style: "unit",
                  unit: "megabyte",
                  maximumFractionDigits: 1,
                }).format(b.bytes / 1048576)}
              </p>
              <code className="backup-id" translate="no">
                {b.id}
              </code>
            </div>
            <div className="database-controls">
              <button
                disabled={busy || !backend?.chooseBackup}
                onClick={() => void archive(true, b.id)}
              >
                {t("services.export")}
              </button>
              <button
                disabled={busy || active || !row.database}
                onClick={() =>
                  show({
                    action: "restore_backup",
                    id: p.id,
                    backup: b.id,
                    consent: true,
                  })
                }
              >
                {t("services.restore")}
              </button>
            </div>
          </div>
        ))}
      </section>
      {confirm && (
        <WorkspaceModal
          title={t("services.confirmTitle")}
          busy={busy}
          onClose={() => setConfirm(null)}
        >
          <p>
            {t(
              confirm.action === "restore_backup"
                ? "services.restoreHint"
                : confirm.action === "install_redis_client"
                  ? "services.clientHint"
                  : "services.switchHint",
            )}
          </p>
          {dialogError && <p role="alert">{t("services.operationFailed")}</p>}
          <div className="modal-actions">
            <button
              disabled={busy}
              data-initial-focus
              onClick={() => setConfirm(null)}
            >
              {t("work.cancel")}
            </button>
            <button
              className="primary"
              disabled={busy}
              onClick={() =>
                void (async () => {
                  if (await run(confirm)) setConfirm(null);
                  else setDialogError(true);
                })()
              }
            >
              {t("services.confirm")}
            </button>
          </div>
        </WorkspaceModal>
      )}
    </>
  );
}
