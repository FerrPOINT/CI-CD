// K2: 403 UX — rendered whenever the API answers forbidden for the current
// identity (server-enforced role policy, see AUTHZ_CONTRACT).

import { ResourceStateView, NamespaceLink as Link, Button } from "@pdlc/ui/ui";

import { useTranslation } from "react-i18next";
import { ShieldAlert } from "lucide-react";

export function ForbiddenPage() {
  const { t } = useTranslation();
  return (
    <ResourceStateView
      state={{
        kind: "permission-denied",
        title: t("errors.forbidden.title", "Доступ запрещён"),
        headingLevel: 1,
        icon: <ShieldAlert />,
        message: t(
          "errors.forbidden.description",
          "У вашей роли нет прав на это действие или раздел. Обратитесь к администратору проекта.",
        ),
        back: (
          <Button asChild variant="outline">
            <Link to="/">{t("errors.forbidden.back", "На главную")}</Link>
          </Button>
        ),
      }}
    />
  );
}
