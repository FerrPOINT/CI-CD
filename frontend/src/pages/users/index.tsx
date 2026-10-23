import { ExternalLink, Users } from "lucide-react";
import { PageHeader, Button, usePlatformServices } from "@pdlc/ui/ui";
import { parseNamespaceLocation, withNamespaceLocation } from "@pdlc/ui/lib";
import { useLocation } from "react-router";

export function UsersPage() {
  const { services } = usePlatformServices();
  const { search } = useLocation();
  const adminUrl = services.find(
    (service) => service.key === "admin-panel",
  )?.ui_url;
  const href = adminUrl
    ? withNamespaceLocation(
        `${adminUrl.replace(/\/$/, "")}/users`,
        parseNamespaceLocation(search),
        search,
      )
    : null;
  const label = (
    <>
      Открыть пользователей{" "}
      <ExternalLink className="h-4 w-4" aria-hidden="true" />
    </>
  );
  return (
    <div className="space-y-4">
      <PageHeader
        title="Пользователи платформы"
        leading={
          <>
            <Users className="h-5 w-5" />
          </>
        }
      />
      <p className="text-sm text-text-secondary">
        Учётные записи и личные API-токены управляются в Admin Panel.
      </p>
      <Button asChild={href !== null} disabled={href === null}>
        {href ? <a href={href}>{label}</a> : label}
      </Button>
    </div>
  );
}
