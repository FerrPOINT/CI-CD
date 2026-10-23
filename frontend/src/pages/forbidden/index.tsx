// K2: 403 UX — rendered whenever the API answers forbidden for the current
// identity (server-enforced role policy, see AUTHZ_CONTRACT).

import { PageHeader, NamespaceLink as Link, Button } from "@pdlc/ui/ui";

import { useTranslation } from "react-i18next";
import { ShieldAlert } from "lucide-react";

export function ForbiddenPage() {
  const { t } = useTranslation();
  return (
    <div className="flex min-h-[60vh] flex-col items-center justify-center gap-4 p-8 text-center">
      <ShieldAlert className="h-12 w-12 text-status-failed" aria-hidden />
      <PageHeader title={t("errors.forbidden.title", "Доступ запрещён")} />
      <p className="max-w-md text-sm text-text-secondary">
        {t(
          "errors.forbidden.description",
          "У вашей роли нет прав на это действие или раздел. Обратитесь к администратору проекта.",
        )}
      </p>
      <Button asChild variant="outline">
        <Link to="/">{t("errors.forbidden.back", "На главную")}</Link>
      </Button>
    </div>
  );
}
