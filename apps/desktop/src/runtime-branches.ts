import type { Runtime } from "./workspace";

export interface RuntimeBranch {
  id: string;
  kind: string;
  branch: string;
  latest: Runtime;
  members: Runtime[];
  installed: Runtime[];
  updateAvailable: boolean;
  complete: boolean;
}

// staticphp is a reviewed publisher rebuild, not a PHP prerelease. Unknown
// version schemes remain exact rows and never become rolling update candidates.
function release(version: string): number[] | null {
  const match =
    /^v?(\d+)\.(\d+)(?:\.(\d+))?(?:\.(\d+))?(?:-(?:staticphp\d+|\d+))?$/.exec(
      version,
    );
  return match
    ? [
        Number(match[1]),
        Number(match[2]),
        Number(match[3] ?? 0),
        Number(match[4] ?? 0),
      ]
    : null;
}
export function runtimeVersionLabel(version: string): string {
  return release(version)
    ? version.replace(/^v/, "").replace(/-(?:staticphp\d+|\d+)$/, "")
    : version;
}
function branchName(runtime: Runtime): string {
  if (runtime.branch !== undefined) return runtime.branch ?? runtime.version;
  const numbers = release(runtime.version);
  if (!numbers) return runtime.version;
  return runtime.kind === "node" ||
    runtime.kind === "postgresql" ||
    runtime.kind === "postgresql-tools"
    ? String(numbers[0])
    : `${numbers[0]}.${numbers[1]}`;
}
function compare(a: Runtime, b: Runtime): number {
  const av = release(a.version),
    bv = release(b.version);
  if (av && bv) {
    for (let i = 0; i < 4; i++) if (av[i] !== bv[i]) return av[i] - bv[i];
  }
  // Rebuilds only supersede their predecessor through reviewed catalog metadata.
  return (
    Number(!a.replacement) - Number(!b.replacement) ||
    a.version.localeCompare(b.version, undefined, { numeric: true }) ||
    a.id.localeCompare(b.id)
  );
}
export function runtimeBranches(runtimes: Runtime[]): RuntimeBranch[] {
  const groups = new Map<string, Runtime[]>();
  for (const runtime of runtimes) {
    const branch = branchName(runtime);
    const variant = ["php", "postgresql"].includes(runtime.kind)
      ? ""
      : runtime.variant;
    const id = JSON.stringify([runtime.kind, branch, variant]);
    groups.set(id, [...(groups.get(id) ?? []), runtime]);
  }
  return [...groups]
    .map(([id, members]): RuntimeBranch => {
      const ordered = [...members].sort((a, b) => compare(b, a));
      const canonical = ordered.filter((p) =>
        p.kind === "postgresql"
          ? p.variant === "server"
          : p.kind !== "php" || p.variant !== "fpm",
      );
      const selectable = canonical.length ? canonical : ordered;
      const authoritative = selectable
        .map((runtime) =>
          selectable.find(
            (candidate) => candidate.id === runtime.latest_package,
          ),
        )
        .find((candidate) => candidate !== undefined);
      const latest =
        authoritative ??
        selectable.find((p) => !p.replacement) ??
        selectable[0];
      const installed = ordered.filter((p) => p.installed || p.problem);
      const healthy = installed.filter((p) => p.installed && !p.problem);
      const complete =
        latest.installed &&
        !latest.problem &&
        (latest.kind !== "php" ||
          members
            .filter((p) => p.version === latest.version)
            .every((p) => p.installed && !p.problem));
      return {
        id,
        kind: latest.kind,
        branch: branchName(latest),
        latest,
        members,
        installed,
        complete,
        updateAvailable:
          latest.update_available ??
          (healthy.length > 0 &&
            !complete &&
            healthy.some((p) => p.id !== latest.id)),
      };
    })
    .sort((a, b) => compare(b.latest, a.latest));
}

/** New selections use the latest healthy installed branch build. An existing
 * exact selection stays available until the backend explicitly updates it. */
export function latestInstalledRuntimes(
  runtimes: Runtime[],
  selectedId?: string,
): Runtime[] {
  const result = runtimeBranches(runtimes).flatMap((group) => {
    const healthy = group.members.filter(
      (p) =>
        p.installed &&
        !p.problem &&
        (p.kind !== "php" || p.variant !== "fpm") &&
        (p.kind !== "postgresql" ||
          p.variant === "server" ||
          !group.members.some((m) => m.variant === "server")),
    );
    return healthy.sort((a, b) => compare(b, a)).slice(0, 1);
  });
  const current = runtimes.find(
    (p) => p.id === selectedId && p.installed && !p.problem,
  );
  if (current && !result.some((p) => p.id === current.id)) result.push(current);
  return result;
}

export function preferredRuntimeOptions(
  runtimes: Runtime[],
  selectedId?: string,
): Runtime[] {
  const result = runtimeBranches(runtimes).map((group) => {
    const installed = group.members.filter(
      (p) =>
        p.installed &&
        !p.problem &&
        (p.kind !== "php" || p.variant !== "fpm") &&
        (p.kind !== "postgresql" ||
          p.variant === "server" ||
          !group.members.some((m) => m.variant === "server")),
    );
    return installed.sort((a, b) => compare(b, a))[0] ?? group.latest;
  });
  const current = runtimes.find((p) => p.id === selectedId);
  if (current && !result.some((p) => p.id === current.id)) result.push(current);
  return result;
}
