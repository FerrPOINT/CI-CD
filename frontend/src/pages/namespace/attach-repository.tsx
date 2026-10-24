import { useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  SectionHeader,
  Card,
  CardContent,
  Button,
  Input,
  SearchableSelect,
  Notice,
  FormField,
} from "@pdlc/ui/ui";
import { useSessionCommand, type NamespaceLocation } from "@pdlc/ui/lib";
import { api } from "@/api/client";
import { catalogErrorMessage } from "@/api/catalog-errors";
import { useAuth } from "@/shared/auth/auth-provider";
import type { components } from "@/api/schema";

type Attach = components["schemas"]["AttachRepository"];
export function AttachRepository({
  groupId,
  namespace,
  onAttached,
}: {
  groupId: string;
  namespace: NamespaceLocation;
  onAttached?: () => void;
}) {
  const { session } = useAuth();
  const cache = useQueryClient();
  const [offset, setOffset] = useState(0);
  const [search, setSearch] = useState("");
  const [repository, setRepository] = useState("");
  const [slug, setSlug] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const journal = useSessionCommand<{ repository: string; command: Attach }>(
    `forge:repository-attach:${session?.subject}:${namespace.registry_instance_id}:${namespace.namespace_id}`,
  );
  const available = useQuery({
    queryKey: [
      "unbound-repositories",
      session?.subject,
      namespace.registry_instance_id,
      namespace.namespace_id,
      offset,
      search,
    ],
    queryFn: ({ signal }) =>
      api<components["schemas"]["CatalogPage"]>(
        `/catalog/available-repositories?limit=50&offset=${offset}&search=${encodeURIComponent(search)}`,
        { signal },
      ),
  });
  async function attach(event: FormEvent) {
    event.preventDefault();
    if (!session?.subject) {
      setError("Требуется действующая SSO-сессия");
      return;
    }
    const command = journal.capture({
      repository,
      command: { operation_id: crypto.randomUUID(), group_id: groupId, slug },
    });
    setBusy(true);
    setError(undefined);
    try {
      await api<components["schemas"]["AttachReadback"]>(
        `/catalog/repositories/${command.repository}/group`,
        { method: "PUT", body: JSON.stringify(command.command) },
      );
      journal.clear();
      setRepository("");
      setSlug("");
      onAttached?.();
      await Promise.all([
        cache.invalidateQueries({ queryKey: ["unbound-repositories"] }),
        cache.invalidateQueries({
          queryKey: [
            "namespace-repositories",
            namespace.registry_instance_id,
            namespace.namespace_id,
          ],
        }),
      ]);
    } catch (e) {
      setError(catalogErrorMessage(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <Card asChild>
      <form onSubmit={attach} className="max-w-xl">
        <CardContent className="space-y-4">
          <SectionHeader
            title={<>Подключить существующий репозиторий Forge</>}
          />
          <p className="text-sm text-text-muted">
            История Git, номера PR и прежний clone URL сохраняются.
          </p>
          {available.isError ? (
            <Notice role="alert" tone="danger" appearance="inline">
              Каталог недоступен.
            </Notice>
          ) : (
            <FormField id="existing-repository" label={"Репозиторий"}>
              {(baseFieldAttrs) => (
                <SearchableSelect
                  {...baseFieldAttrs}
                  label="Репозиторий Forge"
                  value={journal.command?.repository ?? repository}
                  disabled={
                    busy || Boolean(journal.command) || available.isPending
                  }
                  onChange={(value) => {
                    setRepository(value);
                    setSlug(
                      available.data?.items.find((r) => r.id === value)?.slug ??
                        "",
                    );
                  }}
                  selectedLabel={
                    journal.command
                      ? "Репозиторий выбранного подключения"
                      : undefined
                  }
                  placeholder="Выберите репозиторий"
                  allowEmpty={false}
                  search={search}
                  onSearch={(value) => {
                    setSearch(value);
                    setOffset(0);
                  }}
                  offset={offset}
                  pageSize={50}
                  total={available.data?.total ?? 0}
                  onPage={setOffset}
                  loading={available.isPending}
                  options={(available.data?.items ?? []).map((repo) => ({
                    value: repo.id,
                    label: repo.public_name,
                  }))}
                />
              )}
            </FormField>
          )}
          <div className="flex gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={() => void available.refetch()}
            >
              Обновить
            </Button>
          </div>
          <FormField id="existing-repository-slug" label={"Имя внутри проекта"}>
            {(baseFieldAttrs) => (
              <Input
                {...baseFieldAttrs}
                required
                value={journal.command?.command.slug ?? slug}
                disabled={busy || Boolean(journal.command)}
                onChange={(e) => setSlug(e.target.value)}
              />
            )}
          </FormField>
          {error && (
            <Notice role="alert" tone="danger" appearance="inline">
              {error}. Повтор использует исходную операцию.
            </Notice>
          )}
          <Button
            disabled={busy || (!journal.command && (!repository || !slug))}
          >
            {busy
              ? "Подключаем…"
              : journal.command
                ? "Повторить исходную операцию"
                : "Подключить"}
          </Button>
        </CardContent>
      </form>
    </Card>
  );
}
