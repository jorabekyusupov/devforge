import { it, expect, vi } from "vitest";
import { render, screen, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { InspectorPage } from "./pages/InspectorPage";
import type { InspectionView, WorkspaceBackend, Snapshot } from "./workspace";
async function fixture(installed = true, count = 0) {
  const state: InspectionView = {
    capture: {
      enabled: false,
      events: [
        {
          sequence: 1,
          received_ms: 1700000000000,
          kind: "query",
          context: "c",
          name: "select * from users where id=?",
          duration_ms: 123,
          data: { connection: "pgsql" },
        },
        {
          sequence: 2,
          received_ms: 1700000000001,
          kind: "dump",
          context: "c",
          name: "array",
          duration_ms: 0,
          data: {
            password: "[REDACTED]",
            text: "<img src=x onerror=alert(1)>",
          },
        },
      ],
    },
    companion_installed: installed,
    profiles: {
      c: {
        captured_queries: 1,
        sql_ms: 123,
        duration_ms: 150,
        peak_memory_bytes: 2097152,
        truncated: false,
        patterns: [
          {
            sql: "select * from users where id=?",
            count: 5,
            total_ms: 123,
            slow: true,
            repeated: true,
          },
        ],
      },
    },
  };
  if (count)
    state.capture.events = Array.from({ length: count }, (_, i) => ({
      ...state.capture.events[0],
      sequence: i + 1,
      name: `query-${i + 1}`,
    }));
  const snapshot: Snapshot = {
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
        state: "stopped",
        serving: null,
        queue_state: null,
        database: null,
      },
    ],
    runtimes: [],
    databases: [],
    logs: [],
  };
  const b: WorkspaceBackend = {
    snapshot: vi.fn(async () => ({ snapshot, busy: false })),
    inspector: vi.fn(async () => structuredClone(state)),
    execute: vi.fn(async () => ({})),
    cancel: vi.fn(async () => {}),
    browse: vi.fn(async () => null),
    open: vi.fn(async () => {}),
  };
  const engine = await createEngine();
  await engine.changeLanguage("en");
  render(
    <I18nextProvider i18n={engine}>
      <InspectorPage backend={b} />
    </I18nextProvider>,
  );
  return { b, state, user: userEvent.setup() };
}
it("filters real command-shaped events, shows context hints, and renders payload as text", async () => {
  const { user } = await fixture();
  await screen.findByRole("button", { name: /select \* from users/ });
  expect(screen.getByText(/potential N\+1/)).toBeInTheDocument();
  await user.selectOptions(
    screen.getByRole("combobox", { name: "Event" }),
    "dump",
  );
  await user.click(screen.getByRole("button", { name: /array/ }));
  expect(screen.getByText(/onerror=alert/)).toBeInTheDocument();
  expect(document.querySelector("img[src=x]")).toBeNull();
  await user.type(screen.getByRole("searchbox"), "no matching text");
  expect(screen.getByText("No matching events")).toBeInTheDocument();
});
it("requires clear confirmation and keeps project files outside the action", async () => {
  const { b, user } = await fixture();
  await screen.findByRole("button", { name: "Clear events" });
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Clear events" })).toBeEnabled(),
  );
  await user.click(screen.getByRole("button", { name: "Clear events" }));
  expect(b.execute).not.toHaveBeenCalled();
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "Clear events",
    }),
  );
  await waitFor(() =>
    expect(b.execute).toHaveBeenCalledWith({
      action: "inspector_clear",
      id: "a",
    }),
  );
});
it("keeps install failure in its confirmation dialog and allows retry", async () => {
  const { b, user } = await fixture(false);
  vi.mocked(b.execute).mockRejectedValueOnce({
    scope: "workspace",
    code: "inspection_stop_first",
  });
  await user.click(
    await screen.findByRole("button", { name: "Install companion" }),
  );
  const d = screen.getByRole("dialog");
  expect(within(d).getByText(/composer.json/)).toBeInTheDocument();
  await user.click(
    within(d).getByRole("button", { name: "Install companion" }),
  );
  expect(await within(d).findByRole("alert")).toHaveTextContent(
    "Stop this project",
  );
  await user.click(
    within(d).getByRole("button", { name: "Install companion" }),
  );
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
});

it("keeps a long event stream paged and reachable by keyboard controls", async () => {
  const { user } = await fixture(true, 60);
  await screen.findByText("query-1", { selector: ".event-select span" });
  expect(document.querySelectorAll(".event-select")).toHaveLength(50);
  await user.click(screen.getByRole("button", { name: "Next" }));
  expect(document.querySelectorAll(".event-select")).toHaveLength(10);
  expect(
    screen.getByText("query-60", { selector: ".event-select span" }),
  ).toBeInTheDocument();
});
