import { useState } from "react";
import {
  PageHeader,
  Button,
  Input,
  NamespaceLink as Link,
  ResourceList,
  ResourceListRow,
  LoadingState,
  ErrorState,
  TextLink,
} from "@pdlc/ui/ui";
import { useConfigurations } from "@/api/workspaces";
export function DeliveryConfigurationsPage() {
  const [offset, setOffset] = useState(0);
  const [search, setSearch] = useState("");
  const configs = useConfigurations(undefined, offset, search);
  return (
    <div className="space-y-5">
      <PageHeader title="Непривязанные CI-конфигурации" />
      <p className="text-text-muted">
        Существующие настройки и история сохранены. Подключите конфигурацию на
        странице нужного репозитория.
      </p>
      <Input
        className="max-w-md"
        aria-label="Поиск конфигураций"
        placeholder="Поиск конфигураций…"
        value={search}
        onChange={(event) => {
          setSearch(event.target.value);
          setOffset(0);
        }}
      />
      {configs.isPending ? (
        <LoadingState message="Загружаем конфигурации…" />
      ) : configs.isError ? (
        <ErrorState message="Конфигурации недоступны." />
      ) : (
        <ResourceList asChild variant="bordered" density="comfortable">
          <ul>
            {configs.data.items.map((config) => (
              <ResourceListRow asChild key={config.id}>
                <li>
                  <TextLink asChild>
                    <Link to={`/delivery-configs/${config.id}/pipelines`}>
                      {config.name}
                    </Link>
                  </TextLink>
                  <p className="break-all text-sm text-text-muted">
                    {config.repository_url}
                  </p>
                </li>
              </ResourceListRow>
            ))}
            {!configs.data.total && (
              <ResourceListRow asChild>
                <li className="text-text-muted">
                  Все конфигурации подключены.
                </li>
              </ResourceListRow>
            )}
          </ul>
        </ResourceList>
      )}
      <div className="flex gap-2">
        <Button
          variant="outline"
          disabled={!offset}
          onClick={() => setOffset(Math.max(0, offset - 50))}
        >
          Назад
        </Button>
        <Button
          variant="outline"
          disabled={!configs.data || offset + 50 >= configs.data.total}
          onClick={() => setOffset(offset + 50)}
        >
          Далее
        </Button>
      </div>
    </div>
  );
}
