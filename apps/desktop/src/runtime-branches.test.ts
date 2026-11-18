import { describe, it, expect } from "vitest";
import type { Runtime } from "./workspace";
import { runtimeBranches, preferredRuntimeOptions } from "./runtime-branches";
const php = (
  id: string,
  version: string,
  installed = false,
  variant = "cli",
): Runtime => ({ id, kind: "php", version, variant, installed });
describe("runtime branches", () => {
  it("combines PHP companions and patches while selecting the numeric latest stable CLI", () => {
    const list = [
      php("old", "8.5.9", true),
      php("old-fpm", "8.5.9", true, "fpm"),
      php("latest", "8.5.12"),
      php("latest-fpm", "8.5.12", false, "fpm"),
      php("preview", "8.5.99-RC1"),
    ];
    const rows = runtimeBranches(list);
    expect(rows).toHaveLength(2);
    const stable = rows.find((row) => row.branch === "8.5")!;
    expect(stable.latest.id).toBe("latest");
    expect(stable.installed).toHaveLength(2);
    expect(stable.updateAvailable).toBe(true);
    expect(
      rows.find((row) => row.branch.includes("RC1"))?.members,
    ).toHaveLength(1);
  });
  it("uses reviewed rebuilds, keeps unknown versions separate and supports Windows PHP default", () => {
    const list = [
      { ...php("withdrawn", "8.5.12"), replacement: "reviewed" },
      php("reviewed", "8.5.12-staticphp20261113"),
      php("windows", "8.4.27", true, "default"),
      php("custom", "custom-build"),
    ];
    const rows = runtimeBranches(list);
    expect(rows.find((row) => row.branch === "8.5")?.latest.id).toBe(
      "reviewed",
    );
    expect(rows.find((row) => row.branch === "8.4")?.latest.id).toBe("windows");
    expect(rows.find((row) => row.branch === "custom-build")?.latest.id).toBe(
      "custom",
    );
  });
  it("respects product branch boundaries and backend update availability", () => {
    const rows = runtimeBranches([
      { ...php("node-old", "v22.9.0", true), kind: "node" },
      { ...php("node-new", "v22.10.0"), kind: "node", update_available: false },
      { ...php("node-next", "v24.1.0"), kind: "node" },
      { ...php("pg-old", "17.5", true), kind: "postgresql" },
      { ...php("pg-new", "17.6"), kind: "postgresql" },
      { ...php("mysql-old", "8.4.10", true), kind: "mysql" },
      { ...php("mysql-other", "8.5.0"), kind: "mysql" },
    ]);
    expect(
      rows.find((row) => row.kind === "node" && row.branch === "22")?.latest.id,
    ).toBe("node-new");
    expect(
      rows.find((row) => row.kind === "node" && row.branch === "22")
        ?.updateAvailable,
    ).toBe(false);
    expect(rows.find((row) => row.kind === "postgresql")?.branch).toBe("17");
    expect(rows.filter((row) => row.kind === "mysql")).toHaveLength(2);
  });
  it("offers one installed build per branch while preserving an existing exact selection", () => {
    const list = [
      php("old", "8.5.9", true),
      php("new", "8.5.12", true),
      php("fpm", "8.5.12", true, "fpm"),
      php("next", "8.6.0"),
    ];
    expect(preferredRuntimeOptions(list).map((p) => p.id)).toEqual([
      "next",
      "new",
    ]);
    expect(preferredRuntimeOptions(list, "old").map((p) => p.id)).toContain(
      "old",
    );
  });
  it("uses the backend branch and latest identity when provided", () => {
    const rows = runtimeBranches([
      {
        ...php("approved", "8.5.12"),
        branch: "8.5",
        latest_package: "approved",
        update_available: true,
      },
      {
        ...php("unselected", "8.5.99"),
        branch: "8.5",
        latest_package: "approved",
      },
      {
        ...php("four-old", "8.4.13.1", true),
        branch: "8.4",
        latest_package: "four-new",
      },
      {
        ...php("four-new", "8.4.13.2"),
        branch: "8.4",
        latest_package: "four-new",
      },
    ]);
    expect(rows.find((row) => row.branch === "8.5")?.latest.id).toBe(
      "approved",
    );
    expect(rows.find((row) => row.branch === "8.4")?.latest.id).toBe(
      "four-new",
    );
  });
});
