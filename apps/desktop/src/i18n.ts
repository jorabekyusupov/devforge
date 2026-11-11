import { createInstance, type i18n } from "i18next";
import { initReactI18next } from "react-i18next";
import sourceRegistry from "../../../shared/locales/registry.json";

export interface Locale {
  id: string;
  name: string;
  direction: string;
  aliases: string[];
}
export interface Registry {
  fallback: string;
  locales: Locale[];
}
export const registry: Registry = sourceRegistry;
const modules = import.meta.glob("../../../shared/locales/*.json", {
  eager: true,
  import: "default",
});
export const catalogs: Record<
  string,
  Record<string, unknown>
> = Object.fromEntries(
  registry.locales.map((locale) => [
    locale.id,
    modules[`../../../shared/locales/${locale.id}.json`] as Record<
      string,
      unknown
    >,
  ]),
);

export async function createEngine(
  config: Registry = registry,
  messages = catalogs,
): Promise<i18n> {
  const engine = createInstance();
  await engine.use(initReactI18next).init({
    resources: Object.fromEntries(
      config.locales.map((locale) => [
        locale.id,
        { translation: messages[locale.id] },
      ]),
    ),
    lng: config.fallback,
    fallbackLng: config.fallback,
    supportedLngs: config.locales.map((locale) => locale.id),
    interpolation: { escapeValue: false },
    returnNull: false,
  });
  return engine;
}
