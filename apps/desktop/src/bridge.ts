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
  bootstrap(): Promise<Bootstrap>;
  save(settings: Preferences): Promise<Bootstrap>;
  onNavigate(callback: (page: string) => void): Promise<() => void>;
}
const ensureDesktop = () => {
  if (!isTauri()) throw "desktop_unavailable";
};
export const desktopBackend: Backend = {
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
