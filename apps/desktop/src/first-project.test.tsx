import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { FirstProjectChecklist } from "./components/FirstProjectChecklist";
import type { Row, Snapshot } from "./workspace";
function fixture() {
  const row: Row = {
    project: {
      id: "project / A",
      name: "Atlas",
      path: "/fixture/Atlas",
      php: "php84",
      node: "node22",
      revision: 1,
      php_constraint: "^8.3",
      node_constraint: "*",
      environment_names: [],
      active: true,
    },
    site: {
      project_id: "project / A",
      domain: "atlas.devforge.test",
      https: false,
      vite: false,
    },
    development: { queue: false },
    state: "stopped",
    serving: null,
    queue_state: null,
    database: null,
  };
  const snapshot: Snapshot = {
    projects: [row],
    runtimes: [
      {
        id: "php84",
        kind: "php",
        version: "8.4",
        variant: "cli",
        installed: true,
      },
      {
        id: "node22",
        kind: "node",
        version: "22",
        variant: "full",
        installed: true,
      },
    ],
    databases: [],
    logs: [],
  };
  return { row, snapshot };
}
async function mount(
  row: Row,
  snapshot: Snapshot,
  options: { busy?: boolean; dirty?: boolean; language?: string } = {},
) {
  const engine = await createEngine();
  await engine.changeLanguage(options.language ?? "en");
  const onTool = vi.fn(),
    openBrowser = vi.fn();
  render(
    <I18nextProvider i18n={engine}>
      <FirstProjectChecklist
        row={row}
        snapshot={snapshot}
        busy={!!options.busy}
        dirty={!!options.dirty}
        onTool={onTool}
        openBrowser={openBrowser}
      />
    </I18nextProvider>,
  );
  const details = screen
    .getByText(engine.t("firstProject.title"))
    .closest("details")!;
  details.open = true;
  return { onTool, openBrowser, engine, details };
}
describe("first project checks", () => {
  it("keeps unknown preparation and HTTP/trace manual instead of claiming success, without requiring optional services", async () => {
    const { row, snapshot } = fixture();
    const { onTool } = await mount(row, snapshot);
    expect(
      screen
        .getByText("Dependencies, environment file and application key")
        .closest("li"),
    ).toHaveTextContent("Not checked");
    expect(screen.getAllByText("Verify with a fresh request")).toHaveLength(2);
    expect(screen.queryByText("Selected database")).not.toBeInTheDocument();
    expect(
      screen.queryByText("Requested queue worker"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Selected mail capture")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Check PHP setup" }));
    expect(onTool).toHaveBeenCalledWith("php");
    expect(
      screen.getByRole("button", { name: "Check page in browser" }),
    ).toBeDisabled();
    expect(screen.getByRole("link", { name: "Inspector" })).toHaveAttribute(
      "href",
      "#inspector?project=project%20%2F%20A",
    );
    expect(
      screen.getByRole("link", {
        name: "Review configuration and local secrets",
      }),
    ).toHaveAttribute("href", "#configuration?project=project%20%2F%20A");
  });
  it("derives optional checks from saved configuration while keeping DB/mail behavior unverified", async () => {
    const { row, snapshot } = fixture();
    row.project.mail = true;
    row.project.services = {
      cache: "database",
      queue: "project",
      redis_instance: null,
    };
    row.development.queue = true;
    row.site!.vite = true;
    row.state = "running";
    row.serving = {
      preferred_url: "http://atlas.localhost:8080",
      http_url: "http://atlas.localhost:8080",
      https_url: null,
      fallback_url: "http://atlas.localhost:8080",
      system_trusted: false,
      certificate_active: false,
      php_ready: true,
      vite_ready: false,
    };
    row.queue_state = "ready";
    const { openBrowser } = await mount(row, snapshot);
    expect(
      screen.getByText("Selected database").closest("li"),
    ).toHaveTextContent("Action needed");
    expect(
      screen.getByText("Selected mail capture").closest("li"),
    ).toHaveTextContent("Configured; verify behavior");
    expect(
      screen.getByText("Requested Vite server").closest("li"),
    ).toHaveTextContent("Action needed");
    expect(
      screen.getByText("Requested queue worker").closest("li"),
    ).toHaveTextContent("Last observed ready");
    expect(screen.getAllByText("Verify with a fresh request")).toHaveLength(2);
    fireEvent.click(
      screen.getByRole("button", { name: "Check page in browser" }),
    );
    expect(openBrowser).toHaveBeenCalledOnce();
  });
  it("does not accept another installed package as the selected runtime and does not mutate while busy/dirty", async () => {
    const { row, snapshot } = fixture();
    snapshot.runtimes[0].id = "php-other";
    const { onTool, openBrowser } = await mount(row, snapshot, {
      busy: true,
      dirty: true,
    });
    expect(
      screen.getByText("Selected PHP and Node packages").closest("li"),
    ).toHaveTextContent("Action needed");
    fireEvent.click(screen.getByRole("button", { name: "Check PHP setup" }));
    expect(onTool).not.toHaveBeenCalled();
    expect(openBrowser).not.toHaveBeenCalled();
  });
  it("requires the exact Composer selection instead of a different installed Composer", async () => {
    const { row, snapshot } = fixture();
    row.project.composer = "composer-selected";
    snapshot.runtimes.push({
      id: "composer-other",
      kind: "composer",
      version: "2",
      variant: "resource",
      installed: true,
    });
    await mount(row, snapshot);
    expect(
      screen.getByText("Selected exact Composer package").closest("li"),
    ).toHaveTextContent("Action needed");
  });
  it.each(["ru", "uz-Latn"])(
    "uses the %s catalog for the same facts and accessible controls",
    async (language) => {
      const { row, snapshot } = fixture();
      const { engine, details } = await mount(row, snapshot, { language });
      expect(
        within(details).getByRole("button", {
          name: engine.t("firstProject.phpAction"),
        }),
      ).toBeEnabled();
      expect(
        within(details).getAllByText(engine.t("firstProject.status.manual")),
      ).toHaveLength(2);
      expect(details).not.toHaveTextContent("firstProject.");
    },
  );
});
