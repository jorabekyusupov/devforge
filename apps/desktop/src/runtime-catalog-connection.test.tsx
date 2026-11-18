import { afterEach, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { WorkspacePage } from "./pages/WorkspacePage";
import type { WorkspaceBackend } from "./workspace";

afterEach(() => localStorage.removeItem("devforge.runtimeCatalog.selection"));
it("shows reviewed branches while disconnected without pretending installation is known", async () => {
  localStorage.removeItem("devforge.runtimeCatalog.selection");
  const api: WorkspaceBackend = {
    snapshot: vi.fn(async () => {
      throw {
        scope: "workspace",
        code: "native_bridge_authentication_required",
      };
    }),
    catalog: vi.fn(async () => [
      {
        id: "php85",
        kind: "php",
        version: "8.5.12",
        variant: "cli",
        installed: false,
      },
      {
        id: "php85fpm",
        kind: "php",
        version: "8.5.12",
        variant: "fpm",
        installed: false,
      },
    ]),
    execute: vi.fn(async () => ({})),
    cancel: vi.fn(async () => {}),
    browse: vi.fn(async () => null),
    open: vi.fn(async () => {}),
    connect: vi.fn(async () => {}),
  };
  const engine = await createEngine();
  await engine.changeLanguage("en");
  render(
    <I18nextProvider i18n={engine}>
      <WorkspacePage backend={api} mode="runtimes" />
    </I18nextProvider>,
  );
  expect(await screen.findByText("PHP 8.5")).toBeInTheDocument();
  const table = screen.getByRole("table");
  expect(
    within(table).getByText("Connect to check installation status."),
  ).toBeInTheDocument();
  expect(within(table).queryByText("Not installed")).not.toBeInTheDocument();
  expect(within(table).getByRole("button", { name: "Install" })).toBeDisabled();
  expect(
    screen.getByRole("checkbox", { name: "Installed only" }),
  ).toBeDisabled();
  expect(await screen.findByRole("button", { name: "Connect" })).toBeEnabled();
  expect(api.execute).not.toHaveBeenCalled();
});
