import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { WorkspacePage } from "./pages/WorkspacePage";
import type { Runtime, WorkspaceBackend } from "./workspace";

const preferenceKey = "devforge.runtimeCatalog.selection";
const packages: Runtime[] = [
  {
    id: "php84",
    kind: "php",
    version: "8.4.27",
    variant: "cli",
    installed: true,
  },
  {
    id: "php85",
    kind: "php",
    version: "8.5.0",
    variant: "cli",
    installed: false,
  },
  {
    id: "node22",
    kind: "node",
    version: "22.23.4",
    variant: "full",
    installed: true,
  },
  {
    id: "mysql84",
    kind: "mysql",
    version: "8.4.11",
    variant: "server",
    installed: true,
  },
  {
    id: "pg17",
    kind: "postgresql",
    version: "17.5",
    variant: "server",
    installed: false,
  },
  {
    id: "garnet",
    kind: "garnet",
    version: "1.0",
    variant: "server",
    installed: true,
  },
  {
    id: "composer",
    kind: "composer",
    version: "2.8",
    variant: "phar",
    installed: true,
  },
  {
    id: "caddy",
    kind: "caddy",
    version: "2.9",
    variant: "full",
    installed: false,
  },
];
async function mount(language = "en") {
  const backend: WorkspaceBackend = {
    snapshot: vi.fn(async () => ({
      snapshot: { projects: [], runtimes: packages, databases: [], logs: [] },
      busy: false,
    })),
    execute: vi.fn(async () => ({})),
    cancel: vi.fn(async () => {}),
    browse: vi.fn(async () => null),
    open: vi.fn(async () => {}),
  };
  const engine = await createEngine();
  await engine.changeLanguage(language);
  const view = render(
    <I18nextProvider i18n={engine}>
      <WorkspacePage backend={backend} mode="runtimes" />
    </I18nextProvider>,
  );
  await screen.findByRole("table");
  return { ...view, backend, user: userEvent.setup() };
}
beforeEach(() => localStorage.removeItem(preferenceKey));
afterEach(() => localStorage.removeItem(preferenceKey));
describe("runtime catalog navigation", () => {
  it("isolates products and composes installed and version filters without running commands", async () => {
    const { user, backend } = await mount();
    expect(screen.queryByText("22.23.4")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("tablist", { name: "Runtime products" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("checkbox", { name: "Installed only" }));
    expect(screen.queryByText("8.5.0")).not.toBeInTheDocument();
    await user.type(screen.getByRole("searchbox"), "8.5");
    expect(screen.getByRole("status")).toHaveTextContent("No versions match");
    await user.click(screen.getByRole("checkbox"));
    expect(screen.getByText("8.5.0")).toBeInTheDocument();
    await user.clear(screen.getByRole("searchbox"));
    await user.click(screen.getByRole("tab", { name: "Databases" }));
    expect(screen.getAllByText("8.4.11")[0]).toBeInTheDocument();
    const products = screen.getByRole("tablist", { name: "Runtime products" });
    await user.click(within(products).getByRole("tab", { name: "PostgreSQL" }));
    expect(screen.getByText("17.5")).toBeInTheDocument();
    expect(screen.queryByText("8.4.11")).not.toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Cache" }));
    expect(
      screen.getAllByText("1.0", { selector: "code" })[0],
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("tablist", { name: "Runtime products" }),
    ).not.toBeInTheDocument();
    expect(backend.execute).not.toHaveBeenCalled();
  });
  it("supports keyboard tabs and restores the selected category and product after remount", async () => {
    const view = await mount();
    const php = screen.getByRole("tab", { name: "PHP" });
    php.focus();
    await view.user.keyboard("{ArrowRight}{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Databases" })).toHaveFocus();
    const pg = screen.getByRole("tab", { name: "PostgreSQL" });
    await view.user.click(pg);
    await view.user.keyboard("{Home}{End}");
    expect(pg).toHaveFocus();
    view.unmount();
    await mount();
    expect(screen.getByRole("tab", { name: "Databases" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("tab", { name: "PostgreSQL" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(await screen.findByText("17.5")).toBeInTheDocument();
  });
  it("ignores invalid saved preferences", async () => {
    localStorage.setItem(
      preferenceKey,
      '{"category":"obsolete","products":null}',
    );
    await mount();
    expect(screen.getByRole("tab", { name: "PHP" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });
  it.each([
    ["ru", "Базы данных", "Только установленные"],
    ["uz-Latn", "Ma’lumotlar bazalari", "Faqat o‘rnatilganlar"],
  ])(
    "localizes category and filter controls in %s",
    async (locale, databases, installed) => {
      await mount(locale);
      expect(screen.getByRole("tab", { name: databases })).toBeInTheDocument();
      expect(
        screen.getByRole("checkbox", { name: installed }),
      ).toBeInTheDocument();
    },
  );
});

it("updates one PHP branch explicitly and never installs a new minor automatically", async () => {
  const original = packages.length;
  packages.push({
    id: "php84-new",
    kind: "php",
    version: "8.4.28",
    variant: "cli",
    installed: false,
    update_available: true,
  });
  try {
    const { user, backend } = await mount();
    const row = screen.getByText("PHP 8.4").closest("tr")!;
    expect(within(row).getByText("8.4.27")).toBeInTheDocument();
    expect(within(row).getByText("8.4.28")).toBeInTheDocument();
    expect(backend.execute).not.toHaveBeenCalled();
    await user.click(within(row).getByRole("button", { name: /^Update$/ }));
    expect(backend.execute).toHaveBeenCalledWith({
      action: "update_runtime",
      package: "php84-new",
    });
  } finally {
    packages.splice(original);
  }
});
