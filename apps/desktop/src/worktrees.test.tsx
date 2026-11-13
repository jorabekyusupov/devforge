import { it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { WorktreesPage } from "./pages/WorktreesPage";
import type { WorkspaceBackend, WorktreeView, Command } from "./workspace";
const record = {
  schema: 1,
  id: "workspace",
  input: {
    source_id: "a",
    name: "Search",
    branch: "feature/search",
    base: "HEAD",
    new_branch: true,
    database: "empty" as const,
    dependencies: true,
    copy_environment: false,
  },
  source_path: "/source",
  common_dir: "/source/.git",
  git_dir: "/source/.git/worktrees/tree",
  commit: "a".repeat(40),
  domain: "search.devforge.test",
  project_id: "b",
  stage: "ready",
  last_error: null,
  configuration: {
    runtimes: { php: "8.5.12", node: "24.22.2", composer: "2.11.2" },
    database: null,
  },
};
function backend(views: WorktreeView[] = []): WorkspaceBackend {
  return {
    snapshot: vi.fn(async () => ({
      busy: false,
      snapshot: {
        projects: [
          {
            project: {
              id: "a",
              name: "Atlas",
              path: "/source",
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
        worktrees: views,
      },
    })),
    execute: vi.fn(async (command: Command) =>
      command.action === "worktree_plan"
        ? {
            commit: "a".repeat(40),
            domain: "search.devforge.test",
            environment_names: [],
            blockers: [],
            digest: "reviewed",
            sqlite: true,
            source_dirty: false,
          }
        : command.action === "worktree_cleanup_plan"
          ? {
              workspace: views[0],
              dirty: false,
              branch: "feature/search",
              head: "a".repeat(40),
              databases: [],
              blockers: [],
              digest: "cleanup-review",
            }
          : { id: "workspace" },
    ),
    cancel: vi.fn(),
    browse: vi.fn(),
    open: vi.fn(),
  };
}
async function show(b: WorkspaceBackend) {
  const i18n = await createEngine();
  await i18n.changeLanguage("en");
  render(
    <I18nextProvider i18n={i18n}>
      <WorktreesPage backend={b} />
    </I18nextProvider>,
  );
  return userEvent.setup();
}
it("requires a reviewed plan and trust before creating an isolated workspace", async () => {
  const b = backend();
  const user = await show(b);
  await screen.findByText("No workspaces yet");
  await user.click(screen.getByRole("button", { name: "New workspace" }));
  let dialog = screen.getByRole("dialog");
  await user.type(within(dialog).getByLabelText("Workspace name"), "Search");
  await user.type(within(dialog).getByLabelText("Branch"), "feature/search");
  await user.click(
    within(dialog).getByRole("button", { name: "Review workspace" }),
  );
  await within(dialog).findByText("search.devforge.test");
  expect(
    within(dialog).getByRole("button", { name: "Create workspace" }),
  ).toBeDisabled();
  await user.click(within(dialog).getByLabelText(/I trust this repository/));
  await user.click(
    within(dialog).getByRole("button", { name: "Create workspace" }),
  );
  await screen.findByText(/Workspace created/);
  expect(b.execute).toHaveBeenLastCalledWith(
    expect.objectContaining({
      action: "worktree_create",
      review: "reviewed",
      trusted: true,
      input: expect.objectContaining({
        branch: "feature/search",
        database: "empty",
        copy_environment: false,
      }),
    }),
  );
});
it("cleanup review cancellation does not stop or unregister a project", async () => {
  const b = backend([{ record, path: "/tree", retained_path: "/retained" }]);
  const user = await show(b);
  await user.click(
    await screen.findByRole("button", { name: "Review cleanup" }),
  );
  const dialog = await screen.findByRole("dialog");
  expect(
    within(dialog).getByRole("button", { name: "Archive workspace" }),
  ).toBeDisabled();
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(b.execute).toHaveBeenCalledTimes(1);
  expect(b.execute).toHaveBeenCalledWith({
    action: "worktree_cleanup_plan",
    id: "workspace",
  });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
