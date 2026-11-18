import {
  useEffect,
  useId,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { RefreshCw, Search } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Runtime } from "../workspace";
import { runtimeBranches, type RuntimeBranch } from "../runtime-branches";
import "./RuntimeCatalog.css";

const categories = ["php", "node", "databases", "cache", "tools"] as const;
type Category = (typeof categories)[number];
const preferenceKey = "devforge.runtimeCatalog.selection";
const productOrder = [
  "mysql",
  "postgresql",
  "redis",
  "garnet",
  "composer",
  "caddy",
];

function categoryFor(kind: string): Category {
  if (kind === "php" || kind === "node") return kind;
  if (kind === "mysql" || kind === "postgresql") return "databases";
  if (kind === "redis" || kind === "garnet") return "cache";
  return "tools";
}

function loadSelection(): {
  category: Category;
  products: Record<string, string>;
} {
  try {
    const saved: unknown = JSON.parse(
      localStorage.getItem(preferenceKey) ?? "null",
    );
    if (
      saved &&
      typeof saved === "object" &&
      "category" in saved &&
      categories.includes(saved.category as Category)
    ) {
      const products =
        "products" in saved &&
        saved.products &&
        typeof saved.products === "object"
          ? Object.fromEntries(
              Object.entries(saved.products).filter(
                ([, value]) => typeof value === "string",
              ),
            )
          : {};
      return { category: saved.category as Category, products };
    }
  } catch {
    // An unavailable store or an obsolete preference must not block the catalog.
  }
  return { category: "php", products: {} };
}

function Tabs({
  items,
  selected,
  onSelect,
  label,
  id,
  panelId,
}: {
  items: { id: string; label: string }[];
  selected: string;
  onSelect: (id: string) => void;
  label: string;
  id: string;
  panelId: string;
}) {
  function navigate(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    let next: number;
    switch (event.key) {
      case "ArrowRight":
        next = (index + 1) % items.length;
        break;
      case "ArrowLeft":
        next = (index - 1 + items.length) % items.length;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = items.length - 1;
        break;
      default:
        return;
    }
    event.preventDefault();
    onSelect(items[next].id);
    event.currentTarget.parentElement
      ?.querySelectorAll<HTMLButtonElement>('[role="tab"]')
      [next]?.focus();
  }
  return (
    <div className="runtime-catalog-tabs" role="tablist" aria-label={label}>
      {items.map((item, index) => (
        <button
          key={item.id}
          type="button"
          role="tab"
          id={`${id}-${item.id}`}
          aria-selected={selected === item.id}
          aria-controls={panelId}
          tabIndex={selected === item.id ? 0 : -1}
          onClick={() => onSelect(item.id)}
          onKeyDown={(event) => navigate(event, index)}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}

export function RuntimeCatalog({
  runtimes,
  busy,
  onRefresh,
  query,
  onQueryChange,
  children,
}: {
  runtimes: Runtime[];
  busy: boolean;
  onRefresh: () => void;
  query: string;
  onQueryChange: (query: string) => void;
  children: (runtimes: RuntimeBranch[]) => ReactNode;
}) {
  const { t, i18n } = useTranslation();
  const [selection, setSelection] = useState(loadSelection);
  const [installedOnly, setInstalledOnly] = useState(false);
  const id = useId();
  const categoryRuntimes = runtimes.filter(
    (runtime) => categoryFor(runtime.kind) === selection.category,
  );
  const products = [
    ...new Set(categoryRuntimes.map((runtime) => runtime.kind)),
  ].sort((a, b) => {
    const rank = (kind: string) =>
      productOrder.includes(kind)
        ? productOrder.indexOf(kind)
        : productOrder.length;
    return rank(a) - rank(b) || a.localeCompare(b);
  });
  const product = products.includes(selection.products[selection.category])
    ? selection.products[selection.category]
    : products[0];
  const productLabel = (kind: string) =>
    i18n.exists(`work.kinds.${kind}`) ? t(`work.kinds.${kind}`) : kind;
  const installationUnknown = categoryRuntimes.some(
    (runtime) => runtime.installation_unknown,
  );
  const filtered = runtimeBranches(categoryRuntimes).filter(
    (group) =>
      group.kind === product &&
      (!installedOnly ||
        group.installed.some((runtime) => runtime.installed)) &&
      `${productLabel(group.kind)} ${group.branch} ${group.members.map((runtime) => `${runtime.version} ${runtime.variant}`).join(" ")}`
        .toLocaleLowerCase()
        .includes(query.trim().toLocaleLowerCase()),
  );
  useEffect(() => {
    try {
      localStorage.setItem(preferenceKey, JSON.stringify(selection));
    } catch {
      /* Preferences are best effort. */
    }
  }, [selection]);
  return (
    <div className="runtime-catalog">
      <Tabs
        items={categories.map((category) => ({
          id: category,
          label: t(`runtimeCatalog.categories.${category}`),
        }))}
        selected={selection.category}
        onSelect={(category) =>
          setSelection((current) => ({
            ...current,
            category: category as Category,
          }))
        }
        label={t("runtimeCatalog.categoryLabel")}
        id={`${id}-category`}
        panelId={`${id}-category-panel`}
      />
      <div
        role="tabpanel"
        id={`${id}-category-panel`}
        aria-labelledby={`${id}-category-${selection.category}`}
        tabIndex={0}
      >
        {products.length > 1 && (
          <Tabs
            items={products.map((kind) => ({
              id: kind,
              label: productLabel(kind),
            }))}
            selected={product}
            onSelect={(kind) =>
              setSelection((current) => ({
                ...current,
                products: { ...current.products, [current.category]: kind },
              }))
            }
            label={t("runtimeCatalog.productLabel")}
            id={`${id}-product`}
            panelId={`${id}-product-panel`}
          />
        )}
        <div
          id={`${id}-product-panel`}
          role={products.length > 1 ? "tabpanel" : undefined}
          aria-labelledby={
            products.length > 1 ? `${id}-product-${product}` : undefined
          }
          tabIndex={products.length > 1 ? 0 : undefined}
        >
          <div className="workspace-toolbar runtime-catalog-toolbar">
            <label className="search-field">
              <Search aria-hidden="true" />
              <span className="sr-only">{t("work.searchRuntimes")}</span>
              <input
                type="search"
                placeholder={t("runtimeCatalog.searchVersions")}
                value={query}
                name="runtime-version"
                autoComplete="off"
                spellCheck={false}
                onChange={(event) => onQueryChange(event.target.value)}
              />
            </label>
            <label className="runtime-installed-filter">
              <input
                type="checkbox"
                disabled={installationUnknown}
                checked={installedOnly && !installationUnknown}
                onChange={(event) => setInstalledOnly(event.target.checked)}
              />
              {t("runtimeCatalog.installedOnly")}
            </label>
            <button type="button" disabled={busy} onClick={onRefresh}>
              <RefreshCw aria-hidden="true" />
              {t("work.refresh")}
            </button>
          </div>
          {filtered.length > 0 ? (
            children(filtered)
          ) : (
            <p className="runtime-catalog-empty" role="status">
              {t("runtimeCatalog.empty")}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
