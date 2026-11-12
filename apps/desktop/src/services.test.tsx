import { it, expect, vi } from "vitest";
import { render, screen, within, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { ProjectServices } from "./components/ProjectServices";
import type { Snapshot } from "./workspace";
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
