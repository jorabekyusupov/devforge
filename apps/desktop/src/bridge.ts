import type { WorkspaceBackend } from "./workspace";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Theme = "system" | "light" | "dark";
export interface Preferences {
  language: string;
  theme: Theme;
}
export interface Bootstrap {
  settings: Preferences;
  resolvedLocale: string;
  direction: string;
  systemLocale: string | null;
  unsupportedPreference: boolean;
  nativeUiSynced: boolean;
  diagnostics: {
    os: string;
    architecture: string;
    appVersion: string;
    databasePath: string;
    schemaVersion: number;
  };
}
export interface Backend {
  workspace?: WorkspaceBackend;
  quit?(): Promise<void>;
  onFailure?(callback: (code: string) => void): Promise<() => void>;
  bootstrap(): Promise<Bootstrap>;
  save(settings: Preferences): Promise<Bootstrap>;
  onNavigate(callback: (page: string) => void): Promise<() => void>;
}
const ensureDesktop = () => {
  if (!isTauri()) throw "desktop_unavailable";
};
export const desktopBackend: Backend = {
  async quit() {
    ensureDesktop();
    return invoke("quit_application");
  },
  async onFailure(callback) {
    if (!isTauri()) return () => {};
    return listen<string>("workspace-exit-error", (e) => callback(e.payload));
  },
  workspace: {
    async configurationSecret(id, reference, value) {
      ensureDesktop();
      return invoke("configuration_secret", { id, reference, value });
    },
    async configuration(id, plan, writeEnv) {
      ensureDesktop();
      return invoke("configuration_read", { id, plan, writeEnv });
    },
    async debugView(id) {
      ensureDesktop();
      return invoke("debug_read", {
        request: { id, kind: "view", profile: null, file: null },
      });
    },
    async debugProfile(id, profile) {
      ensureDesktop();
      return invoke("debug_read", {
        request: { id, kind: "profile", profile, file: null },
      });
    },
    async debugSource(id, file) {
      ensureDesktop();
      return invoke("debug_read", {
        request: { id, kind: "source", profile: null, file },
      });
    },
    async chooseDebugProfile() {
      ensureDesktop();
      return invoke("choose_debug_profile");
    },
    async inbox(id, query, page) {
      ensureDesktop();
      return invoke("activity_read", {
        request: {
          id,
          query,
          page,
          kind: "inbox",
          message: null,
          index: null,
          filter: null,
        },
      });
    },
    async message(id, message) {
      ensureDesktop();
      return invoke("activity_read", {
        request: {
          id,
          message,
          query: "",
          page: 0,
          kind: "message",
          index: null,
          filter: null,
        },
      });
    },
    async part(id, message, index) {
      ensureDesktop();
      return invoke("activity_read", {
        request: {
          id,
          message,
          index,
          query: "",
          page: 0,
          kind: "part",
          filter: null,
        },
      });
    },
    async logs(id, filter) {
      ensureDesktop();
      return invoke("activity_read", {
        request: {
          id,
          filter,
          query: "",
          page: 0,
          kind: "logs",
          message: null,
          index: null,
        },
      });
    },
    async chooseMailFile(name) {
      ensureDesktop();
      return invoke("choose_mail_file", { name });
    },

    async inspector(id, context) {
      ensureDesktop();
      return invoke("inspector_snapshot", { id, context: context ?? null });
    },
    async snapshot() {
      ensureDesktop();
      return invoke("workspace_snapshot");
    },
    async execute(command) {
      ensureDesktop();
      return invoke("workspace_command", { command });
    },
    async cancel() {
      ensureDesktop();
      return invoke("workspace_cancel");
    },
    async chooseBackup(save) {
      ensureDesktop();
      return invoke("choose_backup_file", { save });
    },
    async browse() {
      ensureDesktop();
      return invoke("choose_project_directory");
    },
    async open(id, target) {
      ensureDesktop();
      return invoke("open_project", { id, target });
    },
  },
  async bootstrap() {
    ensureDesktop();
    return invoke<Bootstrap>("bootstrap");
  },
  async save(settings) {
    ensureDesktop();
    return invoke<Bootstrap>("save_settings", { settings });
  },
  async onNavigate(callback) {
    if (!isTauri()) return () => {};
    return listen<string>("native-navigate", (event) =>
      callback(event.payload),
    );
  },
};

/** Opt-in local startup report; production has no browser-automation HTTP endpoint. */
export const reportStartupReady = () => invoke("startup_ready").catch(() => {});
