import { describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { AgentConnections } from "./components/AgentConnections";
import type {
  AgentApi,
  AgentSnapshot,
  AgentPlan,
  AgentConfig,
} from "./agent-api";
function fixture() {
  const view: AgentSnapshot = {
    owner: { state: "connected" },
    projects: [
      { id: "project-a", name: "Atlas" },
      { id: "project-b", name: "Notes" },
    ],
    grants: [],
    plans: [],
    operations: [],
  };
  const config: AgentConfig = {
    grant_id: "grant-one",
    credential_path: "/fixture/private/grant.json",
    config_path: "/fixture/client.json",
    config_snippet:
      '{"command":"/fixture/devforge","args":["mcp","--credential-file","/fixture/private/grant.json"]}',
  };
  const api: AgentApi = {
    state: vi.fn(async () => structuredClone(view)),
    connect: vi.fn(async () => structuredClone(view)),
    issue: vi.fn(async (input) => {
      const grant = {
        grant_id: "grant-one",
        client_id: "client-one",
        project_id: input.project_id,
        scopes: input.scopes,
        created_at: 1000,
        expires_at: 3601000,
        revoked: false,
        expired: false,
      };
      view.grants.push(grant);
      return { grant, config, credential: "NEVER_RENDER_TOKEN" };
    }),
    revoke: vi.fn(async (id) => {
      view.grants.find((g) => g.grant_id === id)!.revoked = true;
      return structuredClone(view);
    }),
    config: vi.fn(async () => config),
    approve: vi.fn(async (digest) => {
      view.plans.find((p) => p.digest === digest)!.state = "approved";
      return structuredClone(view);
    }),
    confirmMachine: vi.fn(async () => structuredClone(view)),
    status: vi.fn(async (ref) =>
      view.operations.find(
        (op) => op.receipt.operation_id === ref.operation_id,
      )!,
    ),
    cancel: vi.fn(async (ref) => {
      const op = view.operations.find(
        (op) => op.receipt.operation_id === ref.operation_id,
      )!;
      op.receipt.state = "cancel_requested";
      return structuredClone(op);
    }),
    reconcile: vi.fn(async (ref) => ({
      grant_id: ref.grant_id,
      receipt: null,
      point_in_time: true as const,
    })),
  };
  return { api, view, config };
}
function plan(): AgentPlan {
  return {
    digest: "d".repeat(64),
    project_id: "project-a",
    project_name: "Atlas",
    project_root: "/fixture/Atlas",
    grant_id: "grant-one",
    client_id: "client-one",
    tool: "domains.apply",
    revision: 3,
    expires_at: 1900000000000,
    state: "pending",
    effects: ["owned_hosts_change"],
    required_scopes: ["sensitive_operation"],
    target: { kind: "domain", action: "hosts" },
    revalidation_required: true,
    machine_confirmation_required: true,
    details: [
      { key: "domain", value: "atlas.devforge.test" },
      { key: "ca_fingerprint", value: "CA_FINGERPRINT" },
      { key: "remaining_ms", value: 60000 },
      { key: "steps.0.operation", value: "hosts_update" },
      { key: "steps.0.target", value: "system_hosts" },
      { key: "hosts.added", value: ["atlas.devforge.test"] },
      { key: "hosts.removed", value: [] },
      { key: "hosts.retained", value: [] },
    ],
  };
}
async function mount(api?: AgentApi, language = "en") {
  const engine = await createEngine();
  await engine.changeLanguage(language);
  const view = render(
    <I18nextProvider i18n={engine}>
      <AgentConnections api={api} />
    </I18nextProvider>,
  );
  return { user: userEvent.setup(), engine, ...view };
}
describe("minimal agent settings", () => {
  it("stays unavailable without native API and does not show fake successful state", async () => {
    await mount();
    expect(
      screen.getByText("Agent controls require the native desktop connection."),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Create grant" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Connected")).not.toBeInTheDocument();
  });
  it("reads status without auto-connecting and requires explicit connect", async () => {
    const { api, view } = fixture();
    view.owner.state = "disconnected";
    vi.mocked(api.connect).mockImplementation(async () => {
      view.owner.state = "connected";
      return structuredClone(view);
    });
    const { user } = await mount(api);
    await screen.findByText("Disconnected");
    expect(api.connect).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Create grant" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Connect manager" }));
    await screen.findByText("Connected");
    expect(api.connect).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "Create grant" })).toBeEnabled();
  });
  it("creates only selected project scopes and TTL, exposing paths instead of a bearer", async () => {
    const { api } = fixture();
    const { user } = await mount(api);
    await screen.findByText("Connected");
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Project" }),
      "project-b",
    );
    await user.click(
      screen.getByRole("checkbox", { name: "Run project code and tests" }),
    );
    await user.click(
      screen.getByRole("checkbox", { name: "Manage project environment" }),
    );
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Access duration" }),
      "604800",
    );
    await user.click(screen.getByRole("button", { name: "Create grant" }));
    await screen.findByRole("textbox", {
      name: "Client configuration snippet",
    });
    expect(api.issue).toHaveBeenCalledWith({
      project_id: "project-b",
      scopes: ["observer", "execution_test", "project_manage"],
      ttl_seconds: 604800,
    });
    expect(document.body).not.toHaveTextContent("NEVER_RENDER_TOKEN");
    expect(
      screen.getByRole("textbox", { name: "Client configuration snippet" }),
    ).toHaveAttribute("readonly");
    expect(screen.getByText("/fixture/private/grant.json")).toBeInTheDocument();
  });
  it("keeps revoke explicit and restores the trigger focus after cancellation", async () => {
    const { api, view } = fixture();
    view.grants = [
      {
        grant_id: "grant-one",
        client_id: "client-one",
        project_id: "project-a",
        scopes: ["observer"],
        created_at: 1000,
        expires_at: 1900000000000,
        revoked: false,
        expired: false,
      },
    ];
    const { user } = await mount(api);
    await screen.findByText("Connected");
    const trigger = screen.getByRole("button", { name: "Revoke" });
    await user.click(trigger);
    expect(api.revoke).not.toHaveBeenCalled();
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Close",
      }),
    );
    expect(trigger).toHaveFocus();
    await user.click(trigger);
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Revoke",
      }),
    );
    await screen.findByText(/^Revoked/);
    expect(api.revoke).toHaveBeenCalledWith("grant-one");
  });
  it("shows exact reviewed plan data and keeps approval separate from machine confirmation", async () => {
    const { api, view } = fixture();
    view.plans = [plan()];
    const { user } = await mount(api);
    await screen.findByText("Connected");
    await user.click(screen.getByRole("button", { name: "Review plan" }));
    let dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("/fixture/Atlas");
    expect(dialog).toHaveTextContent("atlas.devforge.test");
    expect(dialog).toHaveTextContent("CA_FINGERPRINT");
    const approve = within(dialog).getByRole("button", {
      name: "Approve reviewed plan",
    });
    expect(approve).toBeDisabled();
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: "I reviewed this project and the exact changes shown.",
      }),
    );
    await user.click(approve);
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(api.approve).toHaveBeenCalledWith("d".repeat(64));
    expect(api.confirmMachine).not.toHaveBeenCalled();
    await user.click(
      screen.getByRole("button", { name: "Confirm system-change plan" }),
    );
    dialog = screen.getByRole("dialog");
    const confirm = within(dialog).getByRole("button", {
      name: "Confirm system-change plan",
    });
    expect(confirm).toBeDisabled();
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: "I reviewed the exact machine changes shown.",
      }),
    );
    await user.click(confirm);
    await waitFor(() =>
      expect(api.confirmMachine).toHaveBeenCalledWith("d".repeat(64)),
    );
  });
  it("uses the owning grant for operation status/cancel and truthful point-in-time reconciliation", async () => {
    const { api, view } = fixture();
    view.grants = [
      {
        grant_id: "grant-one",
        client_id: "client-one",
        project_id: "project-a",
        scopes: ["observer"],
        created_at: 1000,
        expires_at: 1900000000000,
        revoked: false,
        expired: false,
      },
    ];
    view.operations = [
      {
        grant_id: "grant-one",
        receipt: {
          operation_id: "op-one",
          project_id: "project-a",
          client_id: "client-one",
          state: "running",
        },
      },
    ];
    const { user } = await mount(api);
    await screen.findByText("Connected");
    await user.click(screen.getByRole("button", { name: "Check status" }));
    await waitFor(() =>
      expect(api.status).toHaveBeenCalledWith({
        grant_id: "grant-one",
        operation_id: "op-one",
      }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Request cancellation" }),
      ).toBeEnabled(),
    );
    await user.click(
      screen.getByRole("button", { name: "Request cancellation" }),
    );
    await screen.findByText(/Cancellation requested/);
    expect(api.cancel).toHaveBeenCalledWith({
      grant_id: "grant-one",
      operation_id: "op-one",
    });
    expect(
      screen.queryByText("Cancelled", { exact: true }),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getByText("Reconcile current operation", { selector: "summary" }),
    );
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Grant" }),
      "grant-one",
    );
    await user.type(
      screen.getByRole("textbox", { name: "Idempotency key" }),
      "intent-one",
    );
    await user.click(
      screen.getByRole("button", { name: "Reconcile current operation" }),
    );
    await screen.findByText(/Absence is not a guarantee/);
    expect(api.reconcile).toHaveBeenCalledWith({
      grant_id: "grant-one",
      idempotency_key: "intent-one",
    });
  });
  it("preserves the form on failure and disables mutation until a successful refresh", async () => {
    const { api } = fixture();
    vi.mocked(api.issue).mockRejectedValueOnce(
      new Error("PRIVATE_ERROR_PAYLOAD"),
    );
    const { user } = await mount(api);
    await screen.findByText("Connected");
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Project" }),
      "project-b",
    );
    await user.click(screen.getByRole("button", { name: "Create grant" }));
    await screen.findByRole("alert");
    expect(document.body).not.toHaveTextContent("PRIVATE_ERROR_PAYLOAD");
    expect(screen.getByRole("combobox", { name: "Project" })).toHaveValue(
      "project-b",
    );
    expect(screen.getByRole("button", { name: "Create grant" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Create grant" }),
      ).toBeEnabled(),
    );
  });
  it("requires a nonempty independent scope selection", async () => {
    const { api } = fixture();
    const { user } = await mount(api);
    await screen.findByText("Connected");
    await user.click(
      screen.getByRole("checkbox", { name: "Read project context" }),
    );
    await user.click(screen.getByRole("button", { name: "Create grant" }));
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Select at least one permission",
    );
    expect(api.issue).not.toHaveBeenCalled();
  });
  it("ignores an in-flight issue result after the native API identity changes", async () => {
    const first = fixture();
    const second = fixture();
    let resolveIssue!: (value: Awaited<ReturnType<AgentApi["issue"]>>) => void;
    vi.mocked(first.api.issue).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveIssue = resolve;
        }),
    );
    const { user, engine, rerender } = await mount(first.api);
    await screen.findByText("Connected");
    await user.click(screen.getByRole("button", { name: "Create grant" }));
    rerender(
      <I18nextProvider i18n={engine}>
        <AgentConnections api={second.api} />
      </I18nextProvider>,
    );
    await waitFor(() => expect(second.api.state).toHaveBeenCalledOnce());
    resolveIssue({
      grant: {
        grant_id: "old-grant",
        client_id: "old-client",
        project_id: "project-a",
        scopes: ["observer"],
        created_at: 1,
        expires_at: 2,
        revoked: false,
        expired: false,
      },
      config: first.config,
    });
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Create grant" }),
      ).toBeEnabled(),
    );
    expect(
      screen.queryByRole("textbox", { name: "Client configuration snippet" }),
    ).not.toBeInTheDocument();
  });
  it("restores a stable focus target after an approved dialog closes before delayed state refresh", async () => {
    const { api, view } = fixture();
    view.plans = [plan()];
    let finishRefresh!: (value: AgentSnapshot) => void;
    vi.mocked(api.state)
      .mockResolvedValueOnce(structuredClone(view))
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finishRefresh = resolve;
          }),
      );
    const { user } = await mount(api);
    await screen.findByText("Connected");
    await user.click(screen.getByRole("button", { name: "Review plan" }));
    const dialog = screen.getByRole("dialog");
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: "I reviewed this project and the exact changes shown.",
      }),
    );
    await user.click(
      within(dialog).getByRole("button", { name: "Approve reviewed plan" }),
    );
    await waitFor(() => expect(api.state).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    finishRefresh(structuredClone(view));
    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: "External agents" }),
      ).toHaveFocus(),
    );
  });
  it("fails closed when exact domain review details are unavailable", async () => {
    const { api, view } = fixture();
    const pending = plan();
    pending.details = [];
    view.plans = [pending];
    const { user } = await mount(api);
    await screen.findByText("Connected");
    await user.click(screen.getByRole("button", { name: "Review plan" }));
    const dialog = screen.getByRole("dialog");
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: "I reviewed this project and the exact changes shown.",
      }),
    );
    expect(
      within(dialog).getByRole("button", { name: "Approve reviewed plan" }),
    ).toBeDisabled();
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      "Exact review details",
    );
    expect(api.approve).not.toHaveBeenCalled();
  });
  it("reviews the safe execution invocation and exact context, rejecting count-only execution summaries", async () => {
    const { api, view } = fixture();
    const execution = plan();
    execution.tool = "execution.artisan";
    execution.required_scopes = ["execution_test"];
    execution.machine_confirmation_required = false;
    execution.details = [];
    execution.target = {
      kind: "execution",
      tool: "artisan",
      arg_count: 2,
      managed_test: true,
      timeout_ms: 30000,
      context: {
        environment: true,
        database: false,
        cache: false,
        mail: false,
        test_isolated: false,
      },
      safe_arguments: [],
    };
    view.plans = [execution];
    const { user } = await mount(api);
    await screen.findByText("Connected");
    await user.click(screen.getByRole("button", { name: "Review plan" }));
    let dialog = screen.getByRole("dialog");
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: "I reviewed this project and the exact changes shown.",
      }),
    );
    expect(
      within(dialog).getByRole("button", { name: "Approve reviewed plan" }),
    ).toBeDisabled();
    expect(api.approve).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole("button", { name: "Close" }));
    execution.target.safe_arguments = ["test", "--filter=SafeFixture"];
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Review plan" })).toBeEnabled(),
    );
    await user.click(screen.getByRole("button", { name: "Review plan" }));
    dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("artisan");
    expect(dialog).toHaveTextContent("--filter=SafeFixture");
    expect(dialog).toHaveTextContent("30000");
    expect(dialog).toHaveTextContent("test_isolated: No");
    expect(
      within(dialog).getByRole("button", { name: "Approve reviewed plan" }),
    ).toBeDisabled();
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: "I reviewed this project and the exact changes shown.",
      }),
    );
    await user.click(
      within(dialog).getByRole("button", { name: "Approve reviewed plan" }),
    );
    await waitFor(() =>
      expect(api.approve).toHaveBeenCalledWith(execution.digest),
    );
    expect(api.confirmMachine).not.toHaveBeenCalled();
  });
  it("requires explicit project management for execution and sensitive grant scopes without broadening permissions automatically", async () => {
    const { api } = fixture();
    const { user } = await mount(api);
    await screen.findByText("Connected");
    const management = screen.getByRole("checkbox", {
      name: "Manage project environment",
    });
    await user.click(
      screen.getByRole("checkbox", { name: "Run project code and tests" }),
    );
    expect(management).not.toBeChecked();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "explicitly select project management",
    );
    expect(screen.getByRole("button", { name: "Create grant" })).toBeDisabled();
    await user.click(
      screen.getByRole("checkbox", { name: "Sensitive operations" }),
    );
    expect(management).not.toBeChecked();
    await user.click(screen.getByRole("button", { name: "Create grant" }));
    expect(api.issue).not.toHaveBeenCalled();
    await user.click(management);
    expect(screen.getByRole("button", { name: "Create grant" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Create grant" }));
    await waitFor(() =>
      expect(api.issue).toHaveBeenCalledWith({
        project_id: "project-a",
        scopes: [
          "observer",
          "execution_test",
          "sensitive_operation",
          "project_manage",
        ],
        ttl_seconds: 3600,
      }),
    );
  });
  it.each(["ru", "uz-Latn"])(
    "uses %s catalog for the same native controls",
    async (language) => {
      const { api } = fixture();
      const { engine } = await mount(api, language);
      await screen.findByText(
        engine.t("agentConnections.ownerState.connected"),
      );
      expect(
        screen.getByRole("button", {
          name: engine.t("agentConnections.createGrant"),
        }),
      ).toBeEnabled();
      expect(screen.getAllByRole("checkbox")).toHaveLength(4);
      expect(document.body).not.toHaveTextContent("agentConnections.");
    },
  );
});
