import { describe, it, expect } from "vitest";
import { createEngine, catalogs, registry } from "./i18n";

describe("catalog-backed localization", () => {
  it("changes all three languages and handles Russian plural categories", async () => {
    const engine = await createEngine();
    for (const [id, title] of [
      ["en", "Settings"],
      ["ru", "Настройки"],
      ["uz-Latn", "Sozlamalar"],
    ]) {
      await engine.changeLanguage(id);
      expect(engine.t("settings.title")).toBe(title);
    }
    await engine.changeLanguage("ru");
    expect(engine.t("settings.languageCount", { count: 1 })).toBe(
      "Доступен 1 язык",
    );
    expect(engine.t("settings.languageCount", { count: 2 })).toBe(
      "Доступно 2 языка",
    );
    expect(engine.t("settings.languageCount", { count: 5 })).toBe(
      "Доступно 5 языков",
    );
    expect(engine.t("settings.languageCount", { count: 21 })).toBe(
      "Доступен 21 язык",
    );
  });
  it("falls back for missing messages without replacing the chosen locale", async () => {
    const messages = structuredClone(catalogs);
    delete (messages.ru.settings as Record<string, unknown>).description;
    const engine = await createEngine(registry, messages);
    await engine.changeLanguage("ru");
    expect(engine.t("settings.description")).toBe(
      "A workspace that feels like yours.",
    );
    expect(engine.language).toBe("ru");
  });
  it("accepts an extra locale through data alone", async () => {
    const config = {
      ...registry,
      locales: [
        ...registry.locales,
        { id: "de", name: "Deutsch", direction: "ltr", aliases: [] },
      ],
    };
    const messages = {
      ...catalogs,
      de: {
        ...catalogs.en,
        settings: {
          ...(catalogs.en.settings as object),
          title: "Einstellungen",
        },
      },
    };
    const engine = await createEngine(config, messages);
    await engine.changeLanguage("de");
    expect(engine.t("settings.title")).toBe("Einstellungen");
    expect(engine.t("native.settings")).toBeTruthy();
  });
});
