import { afterEach, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { WorkspacePage } from "./pages/WorkspacePage";
import { runtimeBranches, preferredRuntimeOptions } from "./runtime-branches";
import type { Runtime, WorkspaceBackend } from "./workspace";
const pg = (id: string, variant: string, installed: boolean): Runtime => ({
  id,
  kind: "postgresql",
  variant,
  version: "18.6.0",
  installed,
  latest_package: id,
  update_available: false,
});
afterEach(() => localStorage.removeItem("devforge.runtimeCatalog.selection"));
it("shows one PostgreSQL family and chooses the supported server without requiring the legacy default package", () => {
  const rows = runtimeBranches([
    pg("legacy", "default", false),
    pg("server", "server", true),
  ]);
  expect(rows).toHaveLength(1);
  expect(rows[0].latest.id).toBe("server");
  expect(rows[0].complete).toBe(true);
  expect(rows[0].updateAvailable).toBe(false);
  expect(
    preferredRuntimeOptions([
      pg("legacy", "default", true),
      pg("server", "server", true),
    ]).map((p) => p.id),
  ).toEqual(["server"]);
});
it("labels missing PHP companion as incomplete and offers completion rather than Installed plus Install", async () => {
  localStorage.removeItem("devforge.runtimeCatalog.selection");
  const runtimes: Runtime[] = [
    {
      id: "cli",
      kind: "php",
      variant: "cli",
      version: "8.5.12",
      installed: true,
      update_available: false,
    },
    {
      id: "fpm",
      kind: "php",
      variant: "fpm",
      version: "8.5.12",
      installed: false,
      update_available: false,
    },
  ];
  const backend: WorkspaceBackend = {
    snapshot: vi.fn(async () => ({
      snapshot: { projects: [], runtimes, databases: [], logs: [] },
      busy: false,
    })),
    execute: vi.fn(async () => ({})),
    cancel: vi.fn(async () => {}),
    browse: vi.fn(async () => null),
    open: vi.fn(async () => {}),
  };
  const engine = await createEngine();
  await engine.changeLanguage("en");
  render(
    <I18nextProvider i18n={engine}>
      <WorkspacePage backend={backend} mode="runtimes" />
    </I18nextProvider>,
  );
  const table = await screen.findByRole("table");
  expect(
    within(table).getByText("Incomplete installation"),
  ).toBeInTheDocument();
  expect(
    within(table).queryByText("Installed", { exact: true }),
  ).not.toBeInTheDocument();
  expect(
    within(table).queryByRole("button", { name: /^Install$/ }),
  ).not.toBeInTheDocument();
  await userEvent
    .setup()
    .click(
      within(table).getByRole("button", { name: "Complete installation" }),
    );
  expect(backend.execute).toHaveBeenCalledWith({
    action: "install",
    package: "cli",
  });
});
