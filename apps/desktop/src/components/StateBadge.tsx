import { useTranslation } from "react-i18next";

export function StateBadge({ value }: { value: string }) {
  const { t } = useTranslation();
  return (
    <span className={`state state-${value}`}>
      <i aria-hidden="true" />
      {t(`work.states.${value}`, { defaultValue: value })}
    </span>
  );
}
