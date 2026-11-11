import { useTranslation } from "react-i18next";
import { Folder } from "lucide-react";
export function ProjectsPage() {
  const { t } = useTranslation();
  return (
    <section className="card empty-state">
      <Folder aria-hidden="true" />
      <span className="soon">{t("projects.soon")}</span>
      <h2>{t("projects.empty")}</h2>
      <p>{t("projects.notice")}</p>
      <a className="button" href="#settings">
        {t("projects.openSettings")}
      </a>
    </section>
  );
}
