import { useApplicationAccess } from "@pdlc/ui/auth";
import { usePlatformServices, TextLink } from "@pdlc/ui/ui";
import { withNamespaceLocation, type NamespaceLocation } from "@pdlc/ui/lib";
import { useLocation } from "react-router";
export function WorkspaceLinks({
  namespace,
}: {
  namespace?: NamespaceLocation;
}) {
  const { services } = usePlatformServices();
  const access = useApplicationAccess();
  const all =
    new URLSearchParams(useLocation().search).get("project_scope") === "all";
  const links = namespace
    ? [
        ["task-tracker", "Задачи", "/namespace"],
        ["wiki", "Документы", "/namespace"],
        ["admin-panel", "Подключения", `/namespaces/${namespace.namespace_id}`],
      ]
    : [["admin-panel", "Создать проект", "/namespaces"]];
  return (
    <div className="flex flex-wrap gap-4 text-sm">
      {links
        .filter(
          ([key]) =>
            key !== undefined && access?.permits([key], key === "admin-panel"),
        )
        .map(([key, label, path]) => {
          const origin = services.find(
            (service) => service.key === key,
          )?.ui_url;
          return origin ? (
            <TextLink
              key={key}

              href={withNamespaceLocation(
                origin + path + (all ? "?project_scope=all" : ""),
                namespace ?? null,
              )}
            >
              {label}
            </TextLink>
          ) : (
            <span key={key} className="text-text-muted">
              {label}: недоступно
            </span>
          );
        })}
    </div>
  );
}
