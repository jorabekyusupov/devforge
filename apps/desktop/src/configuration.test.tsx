import { it, expect, vi } from "vitest";
import { render, screen, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { ConfigurationPage } from "./pages/ConfigurationPage";
import type { WorkspaceBackend, ConfigurationDocument } from "./workspace";
it("requires explicit save consent, sends the read revision and preserves rejected edits", async () => {
  const i18n = await createEngine();
  await i18n.changeLanguage("en");
  const doc: ConfigurationDocument = {
    text: "schema: 1\n",
    expected: "original-revision",
    exists: true,
    journal: null,
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
            state: "stopped",
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
    configuration: vi.fn(async () => doc),
    execute: vi.fn(async () => {
      throw { scope: "workspace", code: "manifest_review_changed" };
    }),
    cancel: vi.fn(),
    browse: vi.fn(),
    open: vi.fn(),
  };
  render(
    <I18nextProvider i18n={i18n}>
      <ConfigurationPage backend={backend} />
    </I18nextProvider>,
  );
  const user = userEvent.setup();
  const editor = await screen.findByRole("textbox", {
    name: "Portable YAML configuration",
  });
  await user.type(editor, "# edited");
  await user.click(screen.getByRole("button", { name: "Save configuration" }));
  const modal = screen.getByRole("dialog");
  const save = within(modal).getByRole("button", {
    name: "Save configuration",
  });
  expect(save).toBeDisabled();
  await user.click(within(modal).getByRole("checkbox"));
  await user.click(save);
  await waitFor(() =>
    expect(backend.execute).toHaveBeenCalledWith({
      action: "configuration_save",
      id: "a",
      expected: "original-revision",
      text: "schema: 1\n# edited",
      consent: true,
    }),
  );
  await within(modal).findByRole("alert");
  expect(editor).toHaveValue("schema: 1\n# edited");
  expect(screen.getByRole("dialog")).toBeInTheDocument();
});
