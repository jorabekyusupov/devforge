import { it, expect, vi } from "vitest";
import { render, screen, within, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { WorkspacePage } from "./pages/WorkspacePage";
import { ProjectServices } from "./components/ProjectServices";
import type { Command, Snapshot, WorkspaceBackend } from "./workspace";
const snapshot: Snapshot = {
  projects: [
    {
      project: {
        id: "a",
        name: "Atlas",
        path: "/fixture",
        php: "php",
        node: "node",
        revision: 4,
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
      database: {
        project_id: "a",
        instance_id: "pg",
        database: "original",
        role: "project",
        secret_ref: "reference",
      },
      backups: [
        {
          id: "backup",
          project: "a",
          engine: "postgresql",
          version: "18.6",
          bytes: 100,
          created: 1000,
        },
      ],
    },
  ],
  runtimes: [],
  databases: [
    {
      instance: { id: "pg", package: "pg", port: 54321, ready: true },
      state: "stopped",
      users: 0,
    },
    {
      instance: {
        id: "mysql",
        package: "mysql",
        port: 54322,
        ready: true,
        engine: "mysql",
      },
      state: "stopped",
      users: 0,
    },
  ],
  logs: [],
  caches: [],
};
it("requires a review before changing the database or executing archive SQL", async () => {
  const engine = await createEngine();
  await engine.changeLanguage("en");
  const run = vi.fn(async () => true);
  render(
    <I18nextProvider i18n={engine}>
      <ProjectServices snapshot={snapshot} busy={false} run={run} />
    </I18nextProvider>,
  );
  const user = userEvent.setup();
  expect(screen.getByRole("combobox",{name:"Project"})).toBeInTheDocument();
  fireEvent.change(screen.getByRole("combobox", { name: "Database service" }), {
    target: { value: "mysql" },
  });
  await user.click(screen.getByRole("button", { name: "Switch database" }));
  expect(run).not.toHaveBeenCalled();
  expect(
    within(screen.getByRole("dialog")).getByText(
      /previous database and credentials are retained/,
    ),
  ).toBeInTheDocument();
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }),
  );
  expect(run).not.toHaveBeenCalled();
  await user.click(
    screen.getByRole("button", { name: "Restore to new database" }),
  );
  expect(
    within(screen.getByRole("dialog")).getByText(
      /Only restore a trusted archive/,
    ),
  ).toBeInTheDocument();
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "Continue",
    }),
  );
  expect(run).toHaveBeenCalledWith({
    action: "restore_backup",
    id: "a",
    backup: "backup",
    consent: true,
  });
});
it("requires an owned cache selection for Redis and disables changes for a running project", async () => {
  const engine = await createEngine();
  await engine.changeLanguage("en");
  const run = vi.fn(async () => true);
  const { rerender } = render(
    <I18nextProvider i18n={engine}>
      <ProjectServices snapshot={snapshot} busy={false} run={run} />
    </I18nextProvider>,
  );
  fireEvent.change(screen.getByRole("combobox", { name: "Cache driver" }), {
    target: { value: "redis" },
  });
  expect(screen.getByRole("button", { name: "Save changes" })).toBeDisabled();
  const active = structuredClone(snapshot);
  active.projects[0].state = "running";
  rerender(
    <I18nextProvider i18n={engine}>
      <ProjectServices snapshot={active} busy={false} run={run} />
    </I18nextProvider>,
  );
  expect(screen.getByRole("combobox", { name: "Cache driver" })).toBeDisabled();
  expect(
    screen.getByRole("button", { name: "Restore to new database" }),
  ).toBeDisabled();
});

it("configures stopped database and cache settings and locks editing while running", async () => {
  const engine = await createEngine();
  await engine.changeLanguage("en");
  const current: Snapshot = structuredClone(snapshot);
  current.runtimes = [
    {
      id: "pg",
      kind: "postgresql",
      version: "18.6",
      variant: "server",
      installed: true,
    },
    {
      id: "mysql",
      kind: "mysql",
      version: "8.4.13",
      variant: "server",
      installed: true,
    },
    {
      id: "garnet",
      kind: "garnet",
      version: "2.2.11",
      variant: "server",
      installed: true,
    },
  ];
  current.databases[0].state = "ready";
  current.caches = [
    {
      instance: {
        id: "cache-1",
        project_id: "a",
        package: "garnet",
        port: 6399,
        ready: true,
        settings: { memory_mb: 64, aof_enabled: true },
      },
      state: "stopped",
    },
  ];
  const execute = vi.fn(async (_c: Command) => ({}));
  const backend: WorkspaceBackend = {
    snapshot: vi.fn(async () => ({
      snapshot: structuredClone(current),
      busy: false,
    })),
    execute,
    cancel: vi.fn(async () => {}),
    browse: vi.fn(async () => null),
    open: vi.fn(async () => {}),
  };
  render(
    <I18nextProvider i18n={engine}>
      <WorkspacePage backend={backend} mode="services" />
    </I18nextProvider>,
  );
  const user = userEvent.setup();

  // Create database with custom port.
  const portInput = await screen.findByRole("spinbutton", { name: "Port" });
  await user.type(portInput, "5444");
  await user.click(screen.getByRole("button", { name: "Create service" }));
  expect(execute).toHaveBeenCalledWith({
    action: "create_database",
    package: "mysql",
    port: 5444,
  });

  // Running PostgreSQL opens settings in read-only locked mode.
  const settingsButtons = screen.getAllByRole("button", { name: "Settings" });
  await user.click(settingsButtons[0]);
  let dialog = screen.getByRole("dialog");
  expect(
    within(dialog).getByText(/Stop this service before changing/),
  ).toBeInTheDocument();
  expect(
    within(dialog).getByRole("button", { name: "Save changes" }),
  ).toBeDisabled();
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));

  // Stopped MySQL allows full port + internal settings edit.
  await user.click(settingsButtons[1]);
  dialog = screen.getByRole("dialog");
  const dbPort = within(dialog).getByRole("spinbutton", { name: /^Port/ });
  await user.clear(dbPort);
  await user.type(dbPort, "3307");
  await user.type(
    within(dialog).getByRole("spinbutton", { name: /^Max connections/ }),
    "250",
  );
  await user.type(
    within(dialog).getByRole("spinbutton", { name: /^Memory \/ buffer pool/ }),
    "256",
  );
  await user.type(
    within(dialog).getByRole("spinbutton", { name: /^Max allowed packet/ }),
    "128",
  );
  await user.type(
    within(dialog).getByRole("textbox", { name: /^Timezone/ }),
    "+05:00",
  );
  await user.selectOptions(
    within(dialog).getByRole("combobox", { name: "Query logging" }),
    "all",
  );
  await user.selectOptions(
    within(dialog).getByRole("combobox", { name: "SQL mode" }),
    "relaxed",
  );
  await user.click(
    within(dialog).getByRole("button", { name: "Save changes" }),
  );
  expect(execute).toHaveBeenCalledWith({
    action: "configure_database",
    instance: "mysql",
    port: 3307,
    settings: {
      max_connections: 250,
      memory_mb: 256,
      work_mem_mb: null,
      max_allowed_packet_mb: 128,
      timezone: "+05:00",
      log_statement: "all",
      sql_mode: "relaxed",
    },
  });

  // Stopped Garnet cache allows port, memory, and AOF settings edit.
  await user.click(screen.getByText("Redis-compatible cache"));
  const cacheRow = screen.getByText("127.0.0.1:6399").closest(".service-row") as HTMLElement;
  await user.click(within(cacheRow).getByRole("button", { name: "Settings" }));
  dialog = screen.getByRole("dialog");
  const cachePort = within(dialog).getByRole("spinbutton", { name: /^Port/ });
  await user.clear(cachePort);
  await user.type(cachePort, "6381");
  const cacheMem = within(dialog).getByRole("spinbutton", {
    name: /^Memory limit/,
  });
  await user.clear(cacheMem);
  await user.type(cacheMem, "128");
  await user.click(
    within(dialog).getByRole("switch", { name: "Persist cache to disk (AOF)" }),
  );
  await user.click(
    within(dialog).getByRole("button", { name: "Save changes" }),
  );
  expect(execute).toHaveBeenCalledWith({
    action: "configure_cache",
    instance: "cache-1",
    port: 6381,
    settings: {
      memory_mb: 128,
      aof_enabled: false,
    },
  });
});
