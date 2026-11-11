import { describe, it, expect, vi } from "vitest";
import {
  render,
  screen,
  within,
  waitFor,
  fireEvent,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { WorkspacePage } from "./pages/WorkspacePage";
import type { Snapshot, WorkspaceBackend, Command } from "./workspace";
function backend() {
  let current: Snapshot = {
    projects: [],
    runtimes: [
      {
        id: "php84",
        kind: "php",
        version: "8.4.27",
        variant: "cli",
        installed: true,
      },
      {
        id: "node22",
        kind: "node",
        version: "22.23.4",
        variant: "full",
        installed: true,
      },
    ],
    databases: [],
    logs: [],
  };
  const api: WorkspaceBackend = {
    snapshot: vi.fn(async () => ({
      snapshot: structuredClone(current),
      busy: false,
    })),
    execute: vi.fn(async (command: Command) => {
      if (command.action === "import") {
        current.projects.push({
          project: {
            id: "a",
            name: command.name,
            path: command.path,
            php: command.php,
            node: command.node,
            revision: 1,
            php_constraint: "^8.3",
            node_constraint: "*",
            environment_names: [],
            active: true,
          },
          site: {
            project_id: "a",
            domain: command.domain,
            https: command.https,
            vite: false,
          },
          development: { queue: false },
          state: "stopped",
          serving: null,
          queue_state: null,
          database: null,
        });
      }
      return {};
    }),
    cancel: vi.fn(async () => {}),
    browse: vi.fn(async () => "/fixture/Laravel"),
    open: vi.fn(async () => {}),
  };
  return { api, get: () => current };
}
async function mount(
  b: WorkspaceBackend,
  mode: "projects" | "runtimes" | "services" = "projects",
  language = "en",
) {
  const engine = await createEngine();
  await engine.changeLanguage(language);
  render(
    <I18nextProvider i18n={engine}>
      <WorkspacePage backend={b} mode={mode} />
    </I18nextProvider>,
  );
  return userEvent.setup();
}
describe("daily workspace", () => {
  it("keeps input and localized error inside the import dialog, then recovers through the real command shape", async () => {
    const b = backend();
    const user = await mount(b.api);
    await screen.findByText("Your workspace starts here");
    await user.click(screen.getAllByRole("button", { name: "Add project" })[0]);
    const d = screen.getByRole("dialog");
    await user.type(
      within(d).getByRole("textbox", { name: "Project folder" }),
      "/bad",
    );
    await user.type(
      within(d).getByRole("textbox", { name: "Project name" }),
      "Atlas",
    );
    vi.mocked(b.api.execute).mockRejectedValueOnce({
      scope: "project",
      code: "invalid_project",
    });
    await user.click(within(d).getByRole("button", { name: "Add project" }));
    expect(await within(d).findByRole("alert")).toHaveTextContent(
      "supported Laravel project",
    );
    expect(
      within(d).getByRole("textbox", { name: "Project name" }),
    ).toHaveValue("Atlas");
    await user.clear(
      within(d).getByRole("textbox", { name: "Project folder" }),
    );
    await user.type(
      within(d).getByRole("textbox", { name: "Project folder" }),
      "/valid",
    );
    await user.click(within(d).getByRole("button", { name: "Add project" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(
      await screen.findByRole("heading", { name: "Atlas" }),
    ).toBeInTheDocument();
    expect(b.api.execute).toHaveBeenLastCalledWith(
      expect.objectContaining({
        action: "import",
        path: "/valid",
        domain: "atlas.devforge.test",
      }),
    );
  });
  it("restores focus and permits Escape without importing", async () => {
    const b = backend();
    const user = await mount(b.api);
    const trigger = (
      await screen.findAllByRole("button", { name: "Add project" })
    )[0];
    await user.click(trigger);
    const d = screen.getByRole("dialog");
    fireEvent(d, new Event("cancel", { cancelable: true }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(trigger).toHaveFocus();
    expect(b.api.execute).not.toHaveBeenCalled();
  });
  it("filters actual raw output by stream and text without translating its contents", async () => {
    const b = backend();
    b.get().logs = [
      {
        project_id: "a",
        process: "queue",
        stream: "stdout",
        text: "RAW_JOB_COMPLETE",
      },
      {
        project_id: "a",
        process: "php",
        stream: "stderr",
        text: "RAW_WARNING",
      },
    ];
    const user = await mount(b.api, "services", "ru");
    await screen.findByText("RAW_JOB_COMPLETE");
    await user.type(
      screen.getByRole("searchbox", { name: "Поиск в логах…" }),
      "RAW_WARNING",
    );
    expect(screen.queryByText("RAW_JOB_COMPLETE")).not.toBeInTheDocument();
    expect(screen.getByText("RAW_WARNING")).toBeInTheDocument();
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Поток" }),
      "stdout",
    );
    expect(screen.queryByText("RAW_WARNING")).not.toBeInTheDocument();
  });
  it("cancels a pending install and never announces the package installed before success", async () => {
    const b = backend();
    b.get().runtimes[0].installed = false;
    let reject: (r: unknown) => void = () => {};
    vi.mocked(b.api.execute).mockImplementation(
      () =>
        new Promise((_, r) => {
          reject = r;
        }),
    );
    const user = await mount(b.api, "runtimes");
    await user.click(await screen.findByRole("button", { name: "Install" }));
    expect(
      await screen.findByText("Downloading and verifying runtime…"),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(b.api.cancel).toHaveBeenCalledOnce();
    reject({ scope: "runtime", code: "cancelled" });
    expect(await screen.findByRole("alert")).toHaveTextContent("cancelled");
    expect(screen.getByRole("button", { name: "Install" })).toBeEnabled();
  });
});
