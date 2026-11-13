import { it, expect, vi } from "vitest";
import { render, screen, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { DebuggerPage } from "./pages/DebuggerPage";
import type { WorkspaceBackend, DebugView } from "./workspace";
async function fixture(state = "stopped") {
  const engine = await createEngine();
  await engine.changeLanguage("en");
  const view: DebugView = {
    settings: { mode: "off", runtime: null, breakpoints: [] },
    runtimes: [
      {
        id: "sdk",
        php: "8.5.12",
        xdebug: "3.5.3",
        target: "macos-arm64",
        source_build: true,
        installed: true,
      },
    ],
    session: null,
    request: null,
    profiles: [
      { id: "profile", created_ms: 1000, bytes: 100, oversized: false },
    ],
  };
  const backend: WorkspaceBackend = {
    snapshot: vi.fn(async () => ({
      busy: false,
      snapshot: {
        starter_constraint: "",
        projects: [
          {
            project: {
              id: "a",
              name: "Atlas",
              path: "/fixture",
              php: "php",
              node: "node",
              revision: 1,
              php_constraint: "*",
              node_constraint: "*",
              environment_names: [],
              active: true,
            },
            site: null,
            development: { queue: false },
            state,
            serving: null,
            queue_state: null,
            database: null,
          },
        ],
        runtimes: [],
        databases: [],
        logs: [],
      },
    })),
    execute: vi.fn(async () => ({ configured: true })),
    cancel: vi.fn(),
    browse: vi.fn(),
    open: vi.fn(),
    debugView: vi.fn(async () => view),
    debugProfile: vi.fn(async () => ({
      total_ms: 2,
      memory_bytes: 20,
      truncated: false,
      functions: [
        {
          file: "app/Work.php",
          name: "App\\Work::run",
          self_ms: 1,
          inclusive_ms: 2,
          memory_bytes: 20,
          calls: 1,
        },
      ],
    })),
  };
  render(
    <I18nextProvider i18n={engine}>
      <DebuggerPage backend={backend} />
    </I18nextProvider>,
  );
  await screen.findByRole("button", { name: "Configure Xdebug" });
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Configure Xdebug" }),
    ).toBeEnabled(),
  );
  return { backend, view, user: userEvent.setup() };
}
it("requires reviewed project configuration and retains the dialog during reads", async () => {
  const { backend, user } = await fixture();
  await user.click(screen.getByRole("button", { name: "Configure Xdebug" }));
  const dialog = screen.getByRole("dialog");
  await user.selectOptions(
    within(dialog).getByRole("combobox", { name: "Instrumentation mode" }),
    "debug",
  );
  const save = within(dialog).getByRole("button", { name: "Save changes" });
  expect(save).toBeDisabled();
  await user.click(within(dialog).getByRole("checkbox"));
  await user.click(save);
  expect(backend.execute).toHaveBeenCalledWith(
    expect.objectContaining({
      action: "debug_configure",
      id: "a",
      consent: true,
      settings: { mode: "debug", runtime: "sdk", breakpoints: [] },
    }),
  );
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
});
it("loads a function profile and cancellation preserves retained output", async () => {
  const { backend, user } = await fixture();
  await user.click(screen.getByRole("button", { name: "Function profiles" }));
  await user.click(screen.getByRole("button", { name: /100 B/ }));
  await screen.findByText("App\\Work::run");
  await user.click(screen.getByRole("button", { name: "Delete profile" }));
  expect(backend.execute).not.toHaveBeenCalled();
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }),
  );
  expect(backend.execute).not.toHaveBeenCalled();
  expect(screen.getByText("App\\Work::run")).toBeInTheDocument();
});

it("keeps the latest selected function profile when an earlier read finishes late", async () => {
  const { backend, view, user } = await fixture();
  view.profiles.push({
    id: "newer",
    created_ms: 2000,
    bytes: 200,
    oversized: false,
  });
  let finish: (
    value: Awaited<ReturnType<NonNullable<WorkspaceBackend["debugProfile"]>>>,
  ) => void = () => {};
  const old = new Promise<
    Awaited<ReturnType<NonNullable<WorkspaceBackend["debugProfile"]>>>
  >((resolve) => {
    finish = resolve;
  });
  const latest = {
    total_ms: 2,
    memory_bytes: 2,
    truncated: false,
    functions: [
      {
        file: "New.php",
        name: "LatestFunction",
        self_ms: 2,
        inclusive_ms: 2,
        memory_bytes: 2,
        calls: 1,
      },
    ],
  };
  backend.debugProfile = vi.fn(async (_id, key) =>
    key === "profile" ? old : latest,
  );
  await user.click(screen.getByRole("button", { name: "Function profiles" }));
  await user.click(screen.getByRole("button", { name: /100 B/ }));
  await user.click(screen.getByRole("button", { name: /200 B/ }));
  await screen.findByText("LatestFunction");
  finish({
    ...latest,
    functions: [{ ...latest.functions[0]!, name: "EarlierFunction" }],
  });
  await waitFor(() =>
    expect(screen.queryByText("EarlierFunction")).not.toBeInTheDocument(),
  );
  expect(screen.getByText("LatestFunction")).toBeInTheDocument();
});

it("stops a failed owned session so that the project can recover", async () => {
  const { backend, user } = await fixture("failed");
  await user.click(screen.getByRole("button", { name: "Stop project" }));
  expect(backend.execute).toHaveBeenCalledWith({ action: "stop", id: "a" });
});
