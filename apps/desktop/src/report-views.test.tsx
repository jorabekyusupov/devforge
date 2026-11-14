import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { createEngine } from "./i18n";
import { EnvironmentReportView } from "./components/EnvironmentReportView";
import { StorageReportView } from "./components/StorageReportView";
import type {
  EnvironmentReport,
  RuntimeCleanupPreview,
  StorageInventory,
} from "./components/report-types";
import type { ReactNode } from "react";
function environment(): EnvironmentReport {
  return {
    schema: 1,
    project_id: "project-a",
    project_revision: 2,
    scope: "metadata_only",
    snapshot_semantics: "non_atomic",
    matches_recorded_configuration: true,
    environment_ready: true,
    manifest_sha256: null,
    comparisons: [
      {
        key: "runtimes.php.version",
        expected: "8.5",
        actual: "8.5",
        state: "match",
        reason: "matches_registered_metadata_not_runtime_verification",
        next_action: "none",
      },
    ],
    portability: [
      {
        target: "windows-x64",
        catalog_support: "supported",
        native_support: "unverified",
        scope: "catalog_only",
        reason: "exact_catalog_target_resolution_only",
      },
    ],
    secrets: [
      {
        environment: "API_TOKEN",
        reference_alias: "api",
        registered_name: true,
        availability: "unverified",
        reason:
          "credential_availability_not_queried_registered_name_is_metadata_only",
      },
    ],
    extensions: [
      {
        extension: "pdo_mysql",
        cached_present: true,
        current_support: "unverified",
        evidence:
          "cached_php_diagnostics_without_timestamp_or_revision_not_current_verification",
      },
    ],
    artifact_evidence: {
      schema: 1,
      project_id: "project-a",
      project_revision: 2,
      scope: "metadata",
      snapshot_semantics: "non_atomic",
      catalog_fingerprint: null,
      catalog_provenance: "bundled",
      managed_artifacts_available: true,
      offline_ready: null,
      requirements: [],
      prerequisites: [],
      problems: [],
      registry_complete: true,
      receipt_bytes_read: 128,
    },
    problems: ["secret_availability_not_queried"],
    file_bytes_read: 128,
  };
}
function inventory(): StorageInventory {
  return {
    schema: 1,
    size_semantics: "logical_not_freeable",
    scope_semantics: "owned_namespaces",
    registry_complete: false,
    snapshot_semantics: "non_atomic",
    logical_bytes_observed: 2048,
    entries_examined: 2,
    registry_rows_examined: 1,
    registry_bytes_read: 100,
    measurement_complete: false,
    resources: [
      {
        category: "runtime_package",
        resource_id: "package-one",
        project_id: null,
        workspace_id: null,
        ownership: "unknown",
        logical_bytes: null,
        files: 1,
        directories: 1,
        measurement_complete: false,
        active_refs: [],
        reference_coverage: "partial",
        retained_reason: "unknown_owner",
        problems: ["reference_registry_incomplete"],
      },
    ],
    problems: ["registry_snapshot_incomplete_absence_not_established"],
  };
}
function preview(): RuntimeCleanupPreview {
  return {
    schema: 1,
    action: "remove_runtime_preview",
    package_id: "package-one",
    state: "review_candidate",
    descriptor_sha256: null,
    references: [],
    lease: { state: "unheld_at_observation", reason: "not_held" },
    logical_bytes: 2048,
    size_semantics: "logical",
    snapshot_semantics: "non_atomic",
    apply_semantics: "no_apply_token",
    problems: [],
  };
}
async function mount(node: ReactNode, language = "en") {
  const engine = await createEngine();
  await engine.changeLanguage(language);
  render(<I18nextProvider i18n={engine}>{node}</I18nextProvider>);
  return engine;
}
describe("metadata report presentation", () => {
  it("does not turn unloaded, loading or failed reads into successful empty reports", async () => {
    await mount(
      <>
        <EnvironmentReportView report={null} loading={true} />
        <StorageReportView
          inventory={null}
          loading={false}
          errorCode="read_unavailable"
        />
      </>,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Reading report");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "could not be refreshed",
    );
    expect(screen.queryByText("Matches metadata only")).not.toBeInTheDocument();
    expect(
      screen.queryByText("Complete within reported scope"),
    ).not.toBeInTheDocument();
  });
  it("keeps metadata match, secret registration, cached extensions and catalog support separate from native health", async () => {
    await mount(
      <EnvironmentReportView report={environment()} loading={false} />,
    );
    expect(screen.getByText("Matches metadata only")).toBeInTheDocument();
    expect(
      screen.getByText("Not established by this metadata report"),
    ).toBeInTheDocument();
    expect(screen.getByText("API_TOKEN").closest("li")).toHaveTextContent(
      "Name registered · Unverified",
    );
    expect(screen.getByText("pdo_mysql").closest("li")).toHaveTextContent(
      "Present in cached check · Unverified",
    );
    const target = screen.getByText("windows-x64").closest("tr")!;
    expect(target).toHaveTextContent(
      "Supported by reported evidenceUnverified",
    );
    expect(screen.queryByText("Environment ready")).not.toBeInTheDocument();
  });
  it("shows actual recorded differences and escapes hostile metadata while keeping unknown reasons optional", async () => {
    const report = environment();
    report.comparisons[0] = {
      key: "project.name",
      expected: "<script>PRIVATE_MARKER</script>",
      actual: null,
      state: "missing",
      reason: "new_unmapped_reason",
      next_action: "review_environment_plan",
    };
    report.problems = [];
    await mount(<EnvironmentReportView report={report} loading={false} />);
    expect(
      screen.getByText("<script>PRIVATE_MARKER</script>"),
    ).toBeInTheDocument();
    expect(document.querySelector("script")).toBeNull();
    expect(
      screen.getByText("new_unmapped_reason").closest("details"),
    ).not.toHaveAttribute("open");
    expect(
      screen.getByText(/Additional metadata or ownership checks/),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: "Repository expects" }),
    ).toBeInTheDocument();
  });
  it("does not render fields from an unsupported report schema", async () => {
    const report = environment();
    report.schema = 2;
    await mount(<EnvironmentReportView report={report} loading={false} />);
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.queryByText("project-a")).not.toBeInTheDocument();
  });
  it("labels previous observations during refresh and disables only the read refresh control", async () => {
    const refresh = vi.fn();
    await mount(
      <EnvironmentReportView
        report={environment()}
        loading={true}
        onRefresh={refresh}
      />,
    );
    expect(
      screen.getByText(/Showing a previous observation/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Refresh report" }));
    expect(refresh).not.toHaveBeenCalled();
  });
  it("keeps incomplete logical byte inventory and unknown references explicit", async () => {
    await mount(<StorageReportView inventory={inventory()} loading={false} />);
    expect(
      screen.getByText(/not allocated disk space or freeable space/),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Ownership unknown; do not delete"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/not proof that the resource is unused/),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Incomplete or unverified")).toHaveLength(3);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
  it("presents review-candidate and instantaneous lease as advice without an apply or delete control", async () => {
    await mount(
      <StorageReportView
        inventory={inventory()}
        preview={preview()}
        loading={false}
      />,
    );
    expect(
      screen.getByText("Candidate for further review only"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/not deletion authorization or an apply token/),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Not held at observation time"),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
  it("shows blocked preview and avoids exact byte claims for unsafe integers", async () => {
    const data = inventory();
    data.logical_bytes_observed = Number.MAX_SAFE_INTEGER + 1;
    const review = preview();
    review.state = "blocked";
    review.lease.state = "held";
    review.problems = ["new_unmapped_blocker"];
    const refresh = vi.fn();
    await mount(
      <StorageReportView
        inventory={data}
        preview={review}
        loading={false}
        onRefresh={refresh}
      />,
    );
    expect(screen.getByText("Blocked")).toBeInTheDocument();
    expect(screen.getByText("Held")).toBeInTheDocument();
    expect(screen.queryByText("9,007,199,254,740,992")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Refresh report" }));
    expect(refresh).toHaveBeenCalledOnce();
  });
  it.each(["ru", "uz-Latn"])(
    "localizes %s report semantics while leaving resource IDs literal",
    async (language) => {
      const engine = await mount(
        <>
          <EnvironmentReportView report={environment()} loading={false} />
          <StorageReportView
            inventory={inventory()}
            preview={preview()}
            loading={false}
          />
        </>,
        language,
      );
      expect(
        screen.getByRole("heading", {
          name: engine.t("environmentView.title"),
        }),
      ).toBeInTheDocument();
      expect(
        screen.getByText(engine.t("storageView.previewState.review_candidate")),
      ).toBeInTheDocument();
      expect(screen.getByText("API_TOKEN")).toHaveAttribute("translate", "no");
      expect(screen.getByText("pdo_mysql")).toHaveAttribute("translate", "no");
      expect(document.body).not.toHaveTextContent("environmentView.");
      expect(document.body).not.toHaveTextContent("storageView.");
    },
  );
});
