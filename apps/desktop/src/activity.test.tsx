import { it, expect, vi } from "vitest";
import { render, screen, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { ActivityPage, safeMailHtml } from "./pages/ActivityPage";
import type { WorkspaceBackend } from "./workspace";
it("isolates email HTML and strips active content and remote attributes", () => {
  const safe = safeMailHtml(
    '<h1>Readable</h1><script>parent.evil=1</script><a href="javascript:evil()">link</a><img src="https://example.test/secret" onerror="evil()"><iframe srcdoc="evil"></iframe><form action="http://evil.test"></form>',
  );
  expect(safe).toContain("Readable");
  expect(safe).toContain("default-src 'none'");
  expect(safe).not.toMatch(
    /<script|onerror|https:\/\/example|javascript:|<iframe|<form/,
  );
});
it("previews a mail attachment and confirms destructive inbox actions", async () => {
  const engine = await createEngine();
  await engine.changeLanguage("en");
  const summary = {
    id: "m1",
    received_ms: 1000,
    subject: "Welcome fixture",
    from: "hello@example.test",
    to: "tester@example.test",
    bytes: 200,
    attachments: 1,
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
    execute: vi.fn(async () => ({ deleted: true })),
    cancel: vi.fn(),
    browse: vi.fn(),
    open: vi.fn(),
    inbox: vi.fn(async () => ({
      enabled: true,
      running: true,
      port: 2525,
      inbox: { messages: [summary], total: 1, page: 0 },
    })),
    message: vi.fn(async () => ({
      summary,
      text: "M11_TEXT",
      html: "<h1>Safe preview</h1><script>evil()</script>",
      attachments: [
        { index: 0, name: "note.txt", mime: "text/plain", bytes: 4, cid: null },
      ],
    })),
    part: vi.fn(async () => ({
      attachment: {
        index: 0,
        name: "note.txt",
        mime: "text/plain",
        bytes: 4,
        cid: null,
      },
      base64: btoa("NOTE"),
    })),
  };
  render(
    <I18nextProvider i18n={engine}>
      <ActivityPage backend={backend} />
    </I18nextProvider>,
  );
  const user = userEvent.setup();
  await screen.findByRole("button", { name: /Welcome fixture/ });
  await waitFor(() =>
    expect(screen.getByTitle("Mail HTML preview")).toHaveAttribute(
      "sandbox",
      "",
    ),
  );
  expect(
    screen.getByTitle("Mail HTML preview").getAttribute("srcdoc"),
  ).not.toContain("<script>");
  await user.click(screen.getByRole("button", { name: "Plain text" }));
  expect(screen.getByText("M11_TEXT")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Preview" }));
  expect(
    await within(screen.getByRole("dialog")).findByText("NOTE"),
  ).toBeInTheDocument();
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "Close dialog",
    }),
  );
  await user.click(screen.getByRole("button", { name: "Clear inbox" }));
  expect(backend.execute).not.toHaveBeenCalled();
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "Continue",
    }),
  );
  expect(backend.execute).toHaveBeenCalledWith({
    action: "mail_delete",
    id: "a",
    message: null,
    consent: true,
  });
});
