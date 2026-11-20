import { useState } from "react";
import { useTranslation } from "react-i18next";
import { RotateCcw } from "lucide-react";
import { WorkspaceModal } from "./WorkspaceModal";
import type {
  Cache,
  CacheSettings,
  Command,
  Database,
  DatabaseSettings,
} from "../workspace";

function parseOptionalInt(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const parsed = Number.parseInt(trimmed, 10);
  return Number.isFinite(parsed) ? parsed : null;
}

export function DatabaseSettingsDialog({
  database,
  version,
  busy,
  errorText,
  onClose,
  onSave,
}: {
  database: Database;
  version?: string;
  busy: boolean;
  errorText: string;
  onClose: () => void;
  onSave: (
    command: Extract<Command, { action: "configure_database" }>,
  ) => Promise<boolean>;
}) {
  const { t } = useTranslation();
  const isMysql = database.instance.engine === "mysql";
  const engineLabel = isMysql ? "MySQL" : "PostgreSQL";
  const initial = database.instance.settings ?? {};
  const locked = database.state !== "stopped" || database.users > 0;

  const [port, setPort] = useState(String(database.instance.port));
  const [maxConnections, setMaxConnections] = useState(
    initial.max_connections ? String(initial.max_connections) : "",
  );
  const [memoryMb, setMemoryMb] = useState(
    initial.memory_mb ? String(initial.memory_mb) : "",
  );
  const [workMemMb, setWorkMemMb] = useState(
    initial.work_mem_mb ? String(initial.work_mem_mb) : "",
  );
  const [maxPacketMb, setMaxPacketMb] = useState(
    initial.max_allowed_packet_mb ? String(initial.max_allowed_packet_mb) : "",
  );
  const [timezone, setTimezone] = useState(initial.timezone ?? "");
  const [logStatement, setLogStatement] = useState(
    initial.log_statement ?? "none",
  );
  const [sqlMode, setSqlMode] = useState(initial.sql_mode ?? "strict");

  function resetDefaults() {
    setMaxConnections("");
    setMemoryMb("");
    setWorkMemMb("");
    setMaxPacketMb("");
    setTimezone("");
    setLogStatement("none");
    setSqlMode("strict");
  }

  return (
    <WorkspaceModal
      title={`${t("services.databaseSettingsTitle")} · ${engineLabel}${version ? ` ${version}` : ""}`}
      busy={busy}
      onClose={onClose}
      className="setup-dialog compact service-settings-dialog"
    >
      {locked && (
        <p className="notice" role="status">
          {t("services.stopToEdit")}
        </p>
      )}
      {errorText && (
        <p role="alert" className="modal-error">
          {errorText}
        </p>
      )}
      <p className="muted">{t("services.loopbackHint")}</p>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (locked || busy) return;
          const parsedPort = Number.parseInt(port.trim(), 10);
          if (!Number.isFinite(parsedPort)) return;
          const settings: DatabaseSettings = {
            max_connections: parseOptionalInt(maxConnections),
            memory_mb: parseOptionalInt(memoryMb),
            work_mem_mb: isMysql ? null : parseOptionalInt(workMemMb),
            max_allowed_packet_mb: isMysql
              ? parseOptionalInt(maxPacketMb)
              : null,
            timezone: timezone.trim() || null,
            log_statement: logStatement === "none" ? null : logStatement,
            sql_mode: isMysql && sqlMode !== "strict" ? sqlMode : null,
          };
          const ok = await onSave({
            action: "configure_database",
            instance: database.instance.id,
            port: parsedPort,
            settings,
          });
          if (ok) onClose();
        }}
      >
        <fieldset disabled={busy || locked}>
          <div className="service-settings-grid">
            <label>
              <span>{t("services.port")}</span>
              <input
                name="db-port"
                type="number"
                inputMode="numeric"
                min={1024}
                max={65535}
                required
                value={port}
                onChange={(e) => setPort(e.target.value)}
              />
              <small>{t("services.portRange")}</small>
            </label>
            <label>
              <span>{t("services.maxConnections")}</span>
              <input
                name="db-max-connections"
                type="number"
                inputMode="numeric"
                min={10}
                max={2000}
                placeholder={isMysql ? "151" : "100"}
                value={maxConnections}
                onChange={(e) => setMaxConnections(e.target.value)}
              />
              <small>10–2000</small>
            </label>
            <label>
              <span>{t("services.memoryMb")}</span>
              <input
                name="db-memory-mb"
                type="number"
                inputMode="numeric"
                min={16}
                max={8192}
                placeholder={isMysql ? "64" : "128"}
                value={memoryMb}
                onChange={(e) => setMemoryMb(e.target.value)}
              />
              <small>
                16–8192 MB ({isMysql ? "innodb_buffer_pool_size" : "shared_buffers"})
              </small>
            </label>
            {isMysql ? (
              <label>
                <span>{t("services.maxAllowedPacketMb")}</span>
                <input
                  name="db-max-packet-mb"
                  type="number"
                  inputMode="numeric"
                  min={1}
                  max={1024}
                  placeholder="64"
                  value={maxPacketMb}
                  onChange={(e) => setMaxPacketMb(e.target.value)}
                />
                <small>1–1024 MB</small>
              </label>
            ) : (
              <label>
                <span>{t("services.workMemMb")}</span>
                <input
                  name="db-work-mem-mb"
                  type="number"
                  inputMode="numeric"
                  min={1}
                  max={1024}
                  placeholder="4"
                  value={workMemMb}
                  onChange={(e) => setWorkMemMb(e.target.value)}
                />
                <small>1–1024 MB</small>
              </label>
            )}
            <label>
              <span>{t("services.timezone")}</span>
              <input
                name="db-timezone"
                type="text"
                autoComplete="off"
                spellCheck={false}
                maxLength={80}
                placeholder={isMysql ? "UTC" : "UTC"}
                value={timezone}
                onChange={(e) => setTimezone(e.target.value)}
              />
              <small>
                {t(
                  isMysql
                    ? "services.timezoneHintMysql"
                    : "services.timezoneHintPg",
                )}
              </small>
            </label>
            <label>
              <span>{t("services.logStatement")}</span>
              <select
                name="db-log-statement"
                value={logStatement}
                onChange={(e) => setLogStatement(e.target.value)}
              >
                <option value="none">{t("services.logLevels.none")}</option>
                {!isMysql && (
                  <>
                    <option value="ddl">{t("services.logLevels.ddl")}</option>
                    <option value="mod">{t("services.logLevels.mod")}</option>
                  </>
                )}
                <option value="all">{t("services.logLevels.all")}</option>
              </select>
            </label>
            {isMysql && (
              <label>
                <span>{t("services.sqlMode")}</span>
                <select
                  name="db-sql-mode"
                  value={sqlMode}
                  onChange={(e) => setSqlMode(e.target.value)}
                >
                  <option value="strict">{t("services.sqlModes.strict")}</option>
                  <option value="relaxed">
                    {t("services.sqlModes.relaxed")}
                  </option>
                </select>
              </label>
            )}
          </div>
          <div className="modal-actions service-settings-actions">
            <button type="button" onClick={resetDefaults}>
              <RotateCcw aria-hidden="true" />
              {t("services.resetDefaults")}
            </button>
            <div className="service-settings-submit-group">
              <button type="button" onClick={onClose}>
                {t("work.cancel")}
              </button>
              <button className="primary" type="submit" disabled={locked}>
                {busy ? t("work.working") : t("work.save")}
              </button>
            </div>
          </div>
        </fieldset>
      </form>
    </WorkspaceModal>
  );
}

export function CacheSettingsDialog({
  cache,
  version,
  projectActive,
  busy,
  errorText,
  onClose,
  onSave,
}: {
  cache: Cache;
  version?: string;
  projectActive: boolean;
  busy: boolean;
  errorText: string;
  onClose: () => void;
  onSave: (
    command: Extract<Command, { action: "configure_cache" }>,
  ) => Promise<boolean>;
}) {
  const { t } = useTranslation();
  const initial = cache.instance.settings ?? {};
  const locked = cache.state !== "stopped" || projectActive;

  const [port, setPort] = useState(String(cache.instance.port));
  const [memoryMb, setMemoryMb] = useState(
    initial.memory_mb ? String(initial.memory_mb) : "",
  );
  const [aofEnabled, setAofEnabled] = useState(initial.aof_enabled ?? true);

  return (
    <WorkspaceModal
      title={`${t("services.cacheSettingsTitle")} · Garnet${version ? ` ${version}` : ""}`}
      busy={busy}
      onClose={onClose}
      className="setup-dialog compact service-settings-dialog"
    >
      {locked && (
        <p className="notice" role="status">
          {t("services.stopToEdit")}
        </p>
      )}
      {errorText && (
        <p role="alert" className="modal-error">
          {errorText}
        </p>
      )}
      <p className="muted">{t("services.loopbackHint")}</p>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (locked || busy) return;
          const parsedPort = Number.parseInt(port.trim(), 10);
          if (!Number.isFinite(parsedPort)) return;
          const settings: CacheSettings = {
            memory_mb: parseOptionalInt(memoryMb),
            aof_enabled: aofEnabled,
          };
          const ok = await onSave({
            action: "configure_cache",
            instance: cache.instance.id,
            port: parsedPort,
            settings,
          });
          if (ok) onClose();
        }}
      >
        <fieldset disabled={busy || locked}>
          <div className="service-settings-grid">
            <label>
              <span>{t("services.port")}</span>
              <input
                name="cache-port"
                type="number"
                inputMode="numeric"
                min={1024}
                max={65535}
                required
                value={port}
                onChange={(e) => setPort(e.target.value)}
              />
              <small>{t("services.portRange")}</small>
            </label>
            <label>
              <span>{t("services.cacheMemoryMb")}</span>
              <input
                name="cache-memory-mb"
                type="number"
                inputMode="numeric"
                min={16}
                max={4096}
                placeholder="64"
                value={memoryMb}
                onChange={(e) => setMemoryMb(e.target.value)}
              />
              <small>16–4096 MB</small>
            </label>
          </div>
          <label className="toggle-row service-aof-toggle">
            <span>{t("services.aofPersistence")}</span>
            <input
              type="checkbox"
              role="switch"
              checked={aofEnabled}
              onChange={(e) => setAofEnabled(e.target.checked)}
            />
          </label>
          <div className="modal-actions service-settings-actions">
            <button
              type="button"
              onClick={() => {
                setMemoryMb("");
                setAofEnabled(true);
              }}
            >
              <RotateCcw aria-hidden="true" />
              {t("services.resetDefaults")}
            </button>
            <div className="service-settings-submit-group">
              <button type="button" onClick={onClose}>
                {t("work.cancel")}
              </button>
              <button className="primary" type="submit" disabled={locked}>
                {busy ? t("work.working") : t("work.save")}
              </button>
            </div>
          </div>
        </fieldset>
      </form>
    </WorkspaceModal>
  );
}
