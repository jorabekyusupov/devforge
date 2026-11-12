import { expect, it, vi } from "vitest";
import { render, screen, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { WorkspacePage } from "./pages/WorkspacePage";
import {
  CreateProjectDialog,
  HttpsSetupDialog,
} from "./components/ProjectSetup";
import type {
  Command,
  Row,
  Snapshot,
  WorkspaceBackend,
  HttpsReview,
} from "./workspace";
function row(id: string, name: string): Row {
  return {
    project: {
      id,
      name,
      path: `/fixture/${id}`,
      php: "php",
      node: "node",
      revision: 1,
      php_constraint: "*",
      node_constraint: "*",
      environment_names: [],
      active: true,
    },
    site: {
      project_id: id,
      domain: `${id}.devforge.test`,
      https: true,
      vite: false,
    },
    development: { queue: false },
    state: "stopped",
    serving: null,
    queue_state: null,
    database: null,
    navigation: { favorite: false, last_opened_ms: 1 },
  };
}
function fixture() {
  const snapshot: Snapshot = {
    starter_constraint: "^13.0",
    projects: [],
    runtimes: [
      {
        id: "php",
        kind: "php",
        version: "8.5.12",
        variant: "cli",
        installed: true,
      },
      {
        id: "node",
        kind: "node",
        version: "24.22.2",
        variant: "full",
        installed: true,
      },
    ],
    databases: [],
    logs: [],
  };
  const api: WorkspaceBackend = {
    snapshot: vi.fn(async () => ({
      snapshot: structuredClone(snapshot),
      busy: false,
    })),
    execute: vi.fn(async (c: Command) => {
      if (c.action === "create_project") {
        snapshot.projects.push(row("new", c.input.name));
        return { project_id: "new" };
      }
      if (c.action === "favorite") {
        snapshot.projects.find(
          (p) => p.project.id === c.id,
        )!.navigation!.favorite = c.favorite;
      }
      return {};
    }),
    cancel: vi.fn(async () => {}),
    browse: vi.fn(async () => "/fixture/Ўзбек & ' parent"),
    open: vi.fn(async () => {}),
  };
  return { snapshot, api };
}
async function provider(node: React.ReactNode) {
  const engine = await createEngine();
  await engine.changeLanguage("en");
  render(<I18nextProvider i18n={engine}>{node}</I18nextProvider>);
  return userEvent.setup();
}
async function review(api: WorkspaceBackend) {
  const user = await provider(<WorkspacePage backend={api} mode="projects" />);
  await user.click(await screen.findByRole("button", { name: "New project" }));
  const dialog = screen.getByRole("dialog");
  await user.click(within(dialog).getByRole("button", { name: "Continue" }));
  await user.type(
    within(dialog).getByRole("textbox", { name: "Project name" }),
    "Atlas",
  );
  await user.click(within(dialog).getByRole("button", { name: "Browse…" }));
  expect(
    within(dialog).getByRole("textbox", { name: /Folder name/ }),
  ).toHaveValue("atlas");
  await user.click(within(dialog).getByRole("button", { name: "Continue" }));
  return { user, dialog };
}
it("requires review and trust before real create command shape; preserves explicit runtime and destination choices", async () => {
  const b = fixture();
  const { user, dialog } = await review(b.api);
  expect(b.api.execute).not.toHaveBeenCalled();
  expect(
    within(dialog).getByRole("button", { name: "Create project" }),
  ).toBeDisabled();
  await user.click(
    within(dialog).getByRole("checkbox", { name: /I trust this project/ }),
  );
  await user.click(
    within(dialog).getByRole("button", { name: "Create project" }),
  );
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(b.api.execute).toHaveBeenCalledWith(
    expect.objectContaining({
      action: "create_project",
      input: expect.objectContaining({
        parent: "/fixture/Ўзбек & ' parent",
        folder: "atlas",
        php: "php",
        node: "node",
        source: { kind: "laravel", version: "^13.0" },
        options: expect.objectContaining({
          generate_key: true,
          run_scripts: false,
          initialize_sqlite: true,
          trusted: true,
        }),
      }),
    }),
  );
});
it("keeps a failed destination review editable without losing its source or user choices", async () => {
  const b = fixture();
  vi.mocked(b.api.execute).mockRejectedValue({
    scope: "workspace",
    code: "setup_destination_exists",
  });
  const { user, dialog } = await review(b.api);
  await user.click(
    within(dialog).getByRole("checkbox", { name: /I trust this project/ }),
  );
  await user.click(
    within(dialog).getByRole("button", { name: "Create project" }),
  );
  expect(await within(dialog).findByRole("alert")).toHaveTextContent(
    "destination already exists",
  );
  expect(within(dialog).getByRole("alert")).toHaveFocus();
  await user.click(within(dialog).getByRole("button", { name: "Back" }));
  expect(
    within(dialog).getByRole("textbox", { name: "Project name" }),
  ).toHaveValue("Atlas");
  expect(
    within(dialog).getByRole("textbox", { name: "Parent folder" }),
  ).toHaveValue("/fixture/Ўзбек & ' parent");
});
it("retries a registered partial project through prepare, never reclones or silently changes its destination", async () => {
  const b = fixture();
  vi.mocked(b.api.execute).mockImplementation(async (c: Command) => {
    if (c.action === "create_project") {
      b.snapshot.projects.push(row("partial", "Atlas"));
      throw {
        scope: "workspace",
        code: "setup_composer_failed",
        project_id: "partial",
      };
    }
    return { project_id: "partial" };
  });
  const { user, dialog } = await review(b.api);
  await user.click(
    within(dialog).getByRole("checkbox", { name: /I trust this project/ }),
  );
  await user.click(
    within(dialog).getByRole("button", { name: "Create project" }),
  );
  await within(dialog).findByText(/project was added/);
  expect(within(dialog).getByRole("button", { name: "Back" })).toBeDisabled();
  await user.click(
    within(dialog).getByRole("button", { name: "Continue setup" }),
  );
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(b.api.execute).toHaveBeenLastCalledWith(
    expect.objectContaining({ action: "prepare_project", id: "partial" }),
  );
  expect(
    vi
      .mocked(b.api.execute)
      .mock.calls.filter(([c]) => c.action === "create_project"),
  ).toHaveLength(1);
});
it("keeps cancellation reachable while wizard controls are disabled during a long operation", async () => {
  const b = fixture();
  const cancel = vi.fn();
  const user = await provider(
    <CreateProjectDialog
      runtimes={b.snapshot.runtimes}
      starter="^13.0"
      busy
      error={null}
      operation={{ stage: "clone", project_id: null }}
      browse={b.api.browse}
      run={vi.fn()}
      cancel={cancel}
      onClose={vi.fn()}
      onCreated={vi.fn()}
    />,
  );
  expect(screen.getByText("Cloning the repository…")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Cancel operation" }));
  expect(cancel).toHaveBeenCalledOnce();
  expect(screen.getByRole("button", { name: "Close dialog" })).toBeDisabled();
});
it("filters actual favorites and recency and sends an explicit persistent favorite mutation", async () => {
  const b = fixture();
  const a = row("a", "Atlas"),
    s = row("s", "Studio");
  a.navigation = { favorite: true, last_opened_ms: 10 };
  s.navigation = { favorite: false, last_opened_ms: 20 };
  b.snapshot.projects = [a, s];
  const user = await provider(
    <WorkspacePage backend={b.api} mode="projects" />,
  );
  await screen.findByRole("button", { name: "Recent" });
  await user.click(screen.getByRole("button", { name: "Recent" }));
  const list = screen.getByLabelText("Projects");
  expect(within(list).getAllByRole("link")[0]).toHaveTextContent("Studio");
  await user.click(screen.getByRole("button", { name: "Favorites" }));
  expect(within(list).queryByText("Studio")).not.toBeInTheDocument();
  await user.click(
    screen.getByRole("button", { name: "Remove Atlas from favorites" }),
  );
  await waitFor(() =>
    expect(within(list).queryByText("Atlas")).not.toBeInTheDocument(),
  );
  expect(b.api.execute).toHaveBeenCalledWith({
    action: "favorite",
    id: "a",
    favorite: false,
  });
});
it("requires exact HTTPS review consent and binds it to the reviewed ticket and digest", async () => {
  const run = vi.fn(async () => true);
  const review: HttpsReview = {
    preview: {
      domain: "atlas.devforge.test",
      https_enabled: true,
      fingerprint: "f".repeat(64),
      system_trusted: false,
      hosts_ready: false,
      hosts_path: "/etc/hosts",
      domains: ["atlas.devforge.test"],
      helper_available: true,
      elevation_required: true,
    },
    ticket: "plan-owned",
    digest: "a".repeat(64),
    applied: false,
  };
  const user = await provider(
    <HttpsSetupDialog
      row={row("a", "Atlas")}
      review={review}
      busy={false}
      errorText=""
      run={run}
      onClose={vi.fn()}
    />,
  );
  const button = screen.getByRole("button", { name: "Apply system setup" });
  expect(button).toBeDisabled();
  await user.click(
    screen.getByRole("checkbox", { name: /approve these domain entries/ }),
  );
  await user.click(button);
  expect(run).toHaveBeenCalledWith({
    action: "https_apply",
    id: "a",
    ticket: "plan-owned",
    digest: "a".repeat(64),
    consent: true,
  });
});

it("returns focus to the replacement PHP trigger after saving changes its project revision", async () => {
  const b = fixture();
  b.snapshot.projects = [row("a", "Atlas")];
  vi.mocked(b.api.execute).mockImplementation(async (c: Command) => {
    if (c.action === "php_settings") {
      b.snapshot.projects[0].project.php_settings = c.settings;
      b.snapshot.projects[0].project.revision++;
    }
    return {};
  });
  const user = await provider(
    <WorkspacePage backend={b.api} mode="projects" />,
  );
  await user.click(await screen.findByRole("button", { name: "PHP settings" }));
  const dialog = screen.getByRole("dialog");
  const save = within(dialog).getByRole("button", { name: "Save changes" });
  await waitFor(() => expect(save).toBeEnabled());
  await user.type(
    within(dialog).getByRole("textbox", { name: /Memory limit/ }),
    "512M",
  );
  await user.click(save);
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(screen.getByRole("button", { name: "PHP settings" })).toHaveFocus();
});

it("keeps the opening revision when a concurrent project update arrives during PHP editing", async () => {
  const b = fixture();
  b.snapshot.projects = [row("a", "Atlas")];
  let probes = 0;
  vi.mocked(b.api.execute).mockImplementation(async (c: Command) => {
    if (c.action === "php_diagnostics" && ++probes === 2) {
      b.snapshot.projects[0].project.revision = 2;
    }
    if (
      c.action === "php_settings" &&
      c.revision !== b.snapshot.projects[0].project.revision
    ) {
      throw { scope: "project", code: "stale_revision" };
    }
    return {};
  });
  const user = await provider(
    <WorkspacePage backend={b.api} mode="projects" />,
  );
  await user.click(await screen.findByRole("button", { name: "PHP settings" }));
  const dialog = screen.getByRole("dialog");
  const check = within(dialog).getByRole("button", {
    name: "Check selected PHP",
  });
  await waitFor(() => expect(check).toBeEnabled());
  await user.click(check);
  await waitFor(() => expect(check).toBeEnabled());
  await user.type(
    within(dialog).getByRole("textbox", { name: /Memory limit/ }),
    "512M",
  );
  await user.click(
    within(dialog).getByRole("button", { name: "Save changes" }),
  );
  await within(dialog).findByRole("alert");
  expect(b.api.execute).toHaveBeenLastCalledWith(
    expect.objectContaining({ action: "php_settings", revision: 1 }),
  );
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  expect(b.snapshot.projects[0].project.revision).toBe(2);
});
