import { describe, it, expect, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { ProjectDiscoverySettings } from "./components/ProjectDiscoverySettings";
import type { Backend } from "./bridge";

async function mount(
  save = vi.fn(async () => true),
  extra: Partial<Backend> = {},
) {
  const engine = await createEngine();
  await engine.changeLanguage("en");
  const backend = {
    createFolder: vi.fn(async () => "/projects/new"),
    scan: vi.fn(async () => {}),
    ...extra,
  } as Backend;
  render(
    <I18nextProvider i18n={engine}>
      <ProjectDiscoverySettings
        folder="/projects"
        busy={false}
        save={save}
        backend={backend}
      />
    </I18nextProvider>,
  );
  return { user: userEvent.setup(), save, backend };
}
describe("project folder settings", () => {
  it("keeps a draft until explicit save and rejects blank input", async () => {
    const { user, save } = await mount();
    const input = screen.getByRole("textbox", { name: "Folder path" });
    await user.clear(input);
    expect(screen.getByRole("button", { name: "Save folder" })).toBeDisabled();
    await user.type(input, "/custom/Проекты");
    expect(save).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.getByRole("textbox")).toHaveFocus());
    await user.click(screen.getByRole("button", { name: "Save folder" }));
    await waitFor(() => expect(save).toHaveBeenCalledWith("/custom/Проекты"));
  });
  it("shows failed saves and preserves the user's draft", async () => {
    const save = vi.fn(async () => false);
    const { user } = await mount(save);
    const input = screen.getByRole("textbox", { name: "Folder path" });
    await user.clear(input);
    await user.type(input, "/missing");
    await user.click(screen.getByRole("button", { name: "Save folder" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "could not be updated",
    );
    expect(input).toHaveValue("/missing");
  });
  it("keeps a cancelled picker unchanged and saves a chosen folder only explicitly", async () => {
    const browse = vi
      .fn()
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce("/chosen");
    const { user, save } = await mount(undefined, {
      workspace: { browse } as unknown as Backend["workspace"],
    });
    await user.click(screen.getByRole("button", { name: "Choose folder" }));
    expect(screen.getByRole("textbox")).toHaveValue("/projects");
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Choose folder" }),
      ).toHaveFocus(),
    );
    await user.click(screen.getByRole("button", { name: "Choose folder" }));
    await waitFor(() =>
      expect(screen.getByRole("textbox")).toHaveValue("/chosen"),
    );
    expect(save).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Save folder" }));
    expect(save).toHaveBeenCalledWith("/chosen");
  });
  it("resets the draft to the home default and creates only on explicit action", async () => {
    const { user, save, backend } = await mount();
    await user.click(screen.getByRole("button", { name: "Use ~/Devforge" }));
    expect(screen.getByRole("textbox")).toHaveValue("~/Devforge");
    expect(save).not.toHaveBeenCalled();
    expect(backend.createFolder).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Create folder" }));
    await waitFor(() =>
      expect(backend.createFolder).toHaveBeenCalledWith("~/Devforge"),
    );
    expect(screen.getByRole("textbox")).toHaveValue("/projects/new");
  });
  it("guards unsaved navigation but releases the guard immediately after successful save", async () => {
    const { user } = await mount();
    render(<a href="#projects">Projects</a>);
    await user.clear(screen.getByRole("textbox"));
    await user.type(screen.getByRole("textbox"), "/chosen");
    await user.click(screen.getByRole("link", { name: "Projects" }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Keep editing" }));
    await user.click(screen.getByRole("button", { name: "Save folder" }));
    await user.click(screen.getByRole("link", { name: "Projects" }));
    await waitFor(() => expect(location.hash).toBe("#projects"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
