import { describe, it, expect, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { App } from "./App";
import { createEngine } from "./i18n";
import type { Backend, Bootstrap, Preferences } from "./bridge";

function backend(): Backend & { current: Bootstrap } {
  const service = {
    current: {
      settings: { language: "en", theme: "system" },
      resolvedLocale: "en",
      direction: "ltr",
      systemLocale: "en-US",
      unsupportedPreference: false,
      nativeUiSynced: true,
      diagnostics: {
        os: "macos",
        architecture: "aarch64",
        appVersion: "0.1.0",
        databasePath: "/test/preferences.sqlite3",
        schemaVersion: 1,
      },
    } as Bootstrap,
    bootstrap: vi.fn(async () => structuredClone(service.current)),
    save: vi.fn(async (settings: Preferences) => {
      service.current = {
        ...service.current,
        settings: { ...service.current.settings, ...settings },
        resolvedLocale:
          settings.language === "system" ? "en" : settings.language,
      };
      return structuredClone(service.current);
    }),
    onNavigate: vi.fn(async () => () => {}),
  };
  return service;
}
async function open(service: Backend) {
  window.location.hash = "#settings";
  const engine = await createEngine();
  const view = render(
    <I18nextProvider i18n={engine}>
      <App backend={service} />
    </I18nextProvider>,
  );
  await screen.findByRole("heading", { level: 1 });
  return view;
}
describe("preferences interface", () => {
  it("switches Russian and Uzbek, updates native request data and document language", async () => {
    const service = backend();
    await open(service);
    const user = userEvent.setup();
    await user.click(screen.getByRole("radio", { name: "Русский" }));
    await screen.findByRole("heading", { level: 1, name: "Настройки" });
    expect(service.save).toHaveBeenLastCalledWith({
      language: "ru",
      theme: "system",
    });
    expect(document.documentElement.lang).toBe("ru");
    await user.click(screen.getByRole("radio", { name: "O‘zbekcha" }));
    await screen.findByRole("heading", { level: 1, name: "Sozlamalar" });
    expect(document.documentElement.lang).toBe("uz-Latn");
  });
  it("keeps folder preferences out of unrelated language updates", async () => {
    const service = backend();
    service.current.settings.project_folder = "/temporarily-unavailable";
    await open(service);
    const user = userEvent.setup();
    await user.click(screen.getByRole("radio", { name: "Русский" }));
    await waitFor(() =>
      expect(service.save).toHaveBeenLastCalledWith({
        language: "ru",
        theme: "system",
      }),
    );
    expect(service.current.settings.project_folder).toBe(
      "/temporarily-unavailable",
    );
  });
  it("preserves selected theme and restores preferences on remount", async () => {
    const service = backend();
    const first = await open(service);
    const user = userEvent.setup();
    await user.click(screen.getByRole("radio", { name: "Dark" }));
    await waitFor(() =>
      expect(document.documentElement.dataset.theme).toBe("dark"),
    );
    first.unmount();
    await open(service);
    expect(screen.getByRole("radio", { name: "Dark" })).toBeChecked();
  });
  it("keeps the previous UI state and exposes retry when storage fails", async () => {
    const service = backend();
    vi.mocked(service.save).mockRejectedValue("storage_unavailable");
    await open(service);
    const user = userEvent.setup();
    await user.click(screen.getByRole("radio", { name: "Русский" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Preferences could not be accessed",
    );
    expect(
      screen.getByRole("heading", { name: "Settings", level: 1 }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled();
  });
  it("disables editing when the native connection is unavailable", async () => {
    const service = backend();
    vi.mocked(service.bootstrap).mockRejectedValue("desktop_unavailable");
    await open(service);
    expect(screen.getByRole("radio", { name: "English" })).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Open the DevForge desktop app",
    );
  });
  it("uses system mode as a stored choice without a new language branch", async () => {
    const service = backend();
    await open(service);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("checkbox", { name: "Follow system language" }),
    );
    await waitFor(() =>
      expect(service.current.settings.language).toBe("system"),
    );
    expect(
      screen.getByRole("checkbox", { name: "Follow system language" }),
    ).toBeChecked();
  });
});
