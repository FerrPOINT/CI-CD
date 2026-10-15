import { useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Input,
  Label,
  NamespaceLink as Link,
  SearchableSelect,
  Dialog,
  DialogContent,
  DialogTitle,
  ResourceList,
  ResourceListRow,
} from "@sdlc/ui/ui";
import { useSessionCommand } from "@sdlc/ui/lib";
import { api, ApiError } from "@/api/client";
import { catalogErrorMessage } from "@/api/catalog-errors";
import { useConfigurations, type Configuration } from "@/api/workspaces";
import type { components } from "@/api/schema";
import { useAuth } from "@/shared/auth/auth-provider";
type Command = components["schemas"]["CreateConfig"];
export function RepositoryConfigurations({
  repositoryId,
  writable,
}: {
  repositoryId: string;
  writable: boolean;
}) {
  const { session } = useAuth();
  const [offset, setOffset] = useState(0);
  const configs = useConfigurations(repositoryId, offset);
  const [unboundOffset, setUnboundOffset] = useState(0);
  const [unboundSearch, setUnboundSearch] = useState("");
  const unbound = useConfigurations(undefined, unboundOffset, unboundSearch);
  const [attachConfiguration, setAttachConfiguration] = useState("");
  const [pushDraft, setPushDraft] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [choiceOffset, setChoiceOffset] = useState(0);
  const choices = useQuery({
    queryKey: ["push-config-choices", repositoryId, search, choiceOffset],
    queryFn: ({ signal }) =>
      api<components["schemas"]["ConfigPage"]>(
        `/catalog/repositories/${repositoryId}/delivery-configs?limit=50&offset=${choiceOffset}&search=${encodeURIComponent(search)}`,
        { signal },
      ),
  });
  const push = useQuery({
    queryKey: ["push-config", repositoryId],
    queryFn: ({ signal }) =>
      api<components["schemas"]["PushConfig"]>(
        `/catalog/repositories/${repositoryId}/push-config`,
        { signal },
      ),
  });
  const journal = useSessionCommand<Command>(
    `forge:config:${session?.subject}:${repositoryId}`,
  );
  const [management, setManagement] = useState<
    "create" | "attach" | "push" | null
  >(() => (journal.command ? "create" : null));
  const cache = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const selectedPush = pushDraft ?? push.data?.configuration_id ?? "";
  const selected = useQuery({
    queryKey: ["delivery-config-label", selectedPush],
    enabled: !!selectedPush,
    queryFn: ({ signal }) =>
      api<Configuration>(`/delivery-configurations/${selectedPush}`, {
        signal,
      }),
  });
  const selectedAttach = useQuery({
    queryKey: ["delivery-config-label", attachConfiguration],
    enabled: !!attachConfiguration,
    queryFn: ({ signal }) =>
      api<Configuration>(`/delivery-configurations/${attachConfiguration}`, {
        signal,
      }),
  });
  async function refresh() {
    await Promise.all([
      cache.invalidateQueries({ queryKey: ["delivery-configurations"] }),
      cache.invalidateQueries({
        queryKey: ["push-config-choices", repositoryId],
      }),
      cache.invalidateQueries({ queryKey: ["push-config", repositoryId] }),
      cache.invalidateQueries({ queryKey: ["workspace-summary"] }),
      cache.invalidateQueries({ queryKey: ["execution-empty-configurations"] }),
    ]);
  }
  async function execute(action: () => Promise<unknown>) {
    setBusy(true);
    setError(undefined);
    try {
      await action();
      await refresh();
    } catch (failure) {
      setError(catalogErrorMessage(failure));
    } finally {
      setBusy(false);
    }
  }
  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const command = journal.capture({
      operation_id: crypto.randomUUID(),
      name: String(data.get("name")),
      default_branch: String(data.get("branch")),
    });
    await execute(async () => {
      try {
        await api<Configuration>(
          `/catalog/repositories/${repositoryId}/delivery-config-operations/${command.operation_id}`,
        );
      } catch (failure) {
        if (!(failure instanceof ApiError) || failure.status !== 404)
          throw failure;
        await api<Configuration>(
          `/catalog/repositories/${repositoryId}/delivery-configs`,
          {
            method: "POST",
            body: JSON.stringify(command),
          },
        );
      }
      journal.clear();
      form.reset();
      setManagement(null);
    });
  }
  return (
    <section className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="font-semibold">CI-конфигурации</h2>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            disabled={!writable && !journal.command}
            onClick={() => {
              setError(undefined);
              setManagement("create");
            }}
          >
            {journal.command ? "Проверить создание" : "Создать конфигурацию"}
          </Button>
          {writable && (
            <>
              <Button
                type="button"
                variant="outline"
                onClick={() => {
                  setError(undefined);
                  setManagement("attach");
                }}
              >
                Подключить конфигурацию
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={() => {
                  setError(undefined);
                  setManagement("push");
                }}
              >
                Автозапуск
              </Button>
            </>
          )}
        </div>
      </div>
      {configs.isPending ? (
        <p role="status">Загружаем конфигурации…</p>
      ) : configs.isError ? (
        <p role="alert">Конфигурации недоступны.</p>
      ) : (
        <ResourceList asChild variant="bordered" density="comfortable">
          <ul>
            {configs.data.items.map((config) => (
              <ResourceListRow asChild key={config.id}>
                <li className="flex flex-wrap items-center justify-between gap-3">
                  <div>
                    <p className="font-semibold">{config.name}</p>
                    <p className="text-sm text-text-muted">
                      Ветка: {config.default_branch}
                    </p>
                  </div>
                  <div className="flex flex-wrap gap-3 text-sm">
                    {[
                      ["settings", "Настройки"],
                      ["pipelines", "Запуски"],
                      ["secrets", "Секреты"],
                      ["environments", "Окружения"],
                      ["schedules", "Расписания"],
                      ["webhooks", "Вебхуки"],
                      ["reports", "Отчёты"],
                    ].map(([path, label]) => (
                      <Link
                        key={path}
                        className="text-accent"
                        to={`/delivery-configs/${config.id}/${path}`}
                      >
                        {label}
                      </Link>
                    ))}
                  </div>
                </li>
              </ResourceListRow>
            ))}
            {!configs.data.total && (
              <ResourceListRow asChild>
                <li className="text-text-muted">Конфигураций пока нет.</li>
              </ResourceListRow>
            )}
          </ul>
        </ResourceList>
      )}
      {configs.data && (
        <p className="text-sm text-text-muted">
          {configs.data.total
            ? `${offset + 1}–${offset + configs.data.items.length} из ${configs.data.total}`
            : "0 конфигураций"}
        </p>
      )}
      {(configs.data?.total ?? 0) > 50 && (
        <nav aria-label="Страницы CI-конфигураций" className="flex gap-2">
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
        </nav>
      )}
      {error && management === null && (
        <p role="alert" className="text-danger">
          {error}
        </p>
      )}
      <Dialog
        open={management !== null}
        onOpenChange={(open) => {
          if (!open && !busy) setManagement(null);
        }}
      >
        <DialogContent aria-describedby={undefined}>
          <DialogTitle>
            {management === "create"
              ? "Создать CI-конфигурацию"
              : management === "attach"
                ? "Подключить CI-конфигурацию"
                : "Автозапуск при Git push"}
          </DialogTitle>
          {management === "create" && (
            <form
              onSubmit={create}
              className="max-w-xl space-y-3 rounded-lg border border-border bg-surface p-4"
            >
              <Label htmlFor="config-name">Название</Label>
              <Input
                id="config-name"
                name="name"
                required
                maxLength={255}
                disabled={!writable || busy || !!journal.command}
                defaultValue={journal.command?.name}
              />
              <Label htmlFor="config-branch">Ветка по умолчанию</Label>
              <Input
                id="config-branch"
                name="branch"
                required
                defaultValue={journal.command?.default_branch ?? "main"}
                disabled={!writable || busy || !!journal.command}
              />
              {journal.command && (
                <p className="text-sm text-warning">
                  Сначала проверим результат предыдущей операции.
                </p>
              )}
              <Button disabled={busy || (!writable && !journal.command)}>
                {journal.command ? "Проверить и продолжить" : "Создать"}
              </Button>
            </form>
          )}
          {writable && (
            <>
              {management === "push" && (
                <form
                  className="max-w-xl space-y-3 rounded-lg border border-border bg-surface p-4"
                  onSubmit={(event) => {
                    event.preventDefault();
                    void execute(() =>
                      api(`/catalog/repositories/${repositoryId}/push-config`, {
                        method: "PUT",
                        body: JSON.stringify({
                          configuration_id: selectedPush || null,
                        }),
                      }).then(() => {
                        setPushDraft(null);
                        setManagement(null);
                      }),
                    );
                  }}
                >
                  <p className="text-sm text-text-muted">
                    Push запускает одну выбранную конфигурацию. Остальные
                    доступны вручную и по расписанию.
                  </p>
                  {push.isError || choices.isError ? (
                    <p role="alert">Настройки автозапуска недоступны.</p>
                  ) : !push.data || !choices.data ? (
                    <p role="status">Загружаем настройки…</p>
                  ) : (
                    <label className="block text-sm">
                      Конфигурация
                      <SearchableSelect
                        label="Конфигурация автозапуска"
                        value={selectedPush}
                        selectedLabel={selected.data?.name}
                        placeholder="Отключён"
                        options={choices.data.items.map((config) => ({
                          value: config.id,
                          label: config.name,
                        }))}
                        search={search}
                        onSearch={(value) => {
                          setSearch(value);
                          setChoiceOffset(0);
                        }}
                        offset={choiceOffset}
                        pageSize={50}
                        total={choices.data.total}
                        onPage={setChoiceOffset}
                        onChange={setPushDraft}
                        disabled={busy}
                      />
                    </label>
                  )}
                  <Button
                    disabled={
                      busy ||
                      !push.data ||
                      !choices.data ||
                      selectedPush === (push.data.configuration_id ?? "")
                    }
                  >
                    Сохранить автозапуск
                  </Button>
                </form>
              )}
              {management === "attach" && (
                <form
                  className="max-w-xl space-y-3 rounded-lg border border-border bg-surface p-4"
                  onSubmit={(event) => {
                    event.preventDefault();
                    void execute(() =>
                      api(
                        `/catalog/repositories/${repositoryId}/delivery-configs/${attachConfiguration}`,
                        { method: "PUT" },
                      ).then(() => {
                        setAttachConfiguration("");
                        setManagement(null);
                      }),
                    );
                  }}
                >
                  <p className="text-sm text-text-muted">
                    Прежняя история и настройки сохраняются.
                  </p>
                  {unbound.isError ? (
                    <p role="alert">Конфигурации недоступны.</p>
                  ) : (
                    <label className="block text-sm">
                      Конфигурация
                      <SearchableSelect
                        label="Конфигурация подключения"
                        placeholder="Выберите конфигурацию"
                        allowEmpty={false}
                        value={attachConfiguration}
                        selectedLabel={selectedAttach.data?.name}
                        onChange={setAttachConfiguration}
                        options={(unbound.data?.items ?? []).map((config) => ({
                          value: config.id,
                          label: config.name,
                        }))}
                        loading={unbound.isPending}
                        search={unboundSearch}
                        onSearch={(value) => {
                          setUnboundSearch(value);
                          setUnboundOffset(0);
                        }}
                        offset={unboundOffset}
                        pageSize={50}
                        total={unbound.data?.total ?? 0}
                        onPage={setUnboundOffset}
                        disabled={busy}
                      />
                    </label>
                  )}
                  <div className="flex gap-2">
                    <Button disabled={busy || !attachConfiguration}>
                      Подключить
                    </Button>
                  </div>
                </form>
              )}
            </>
          )}
          {error && (
            <p role="alert" className="text-danger">
              {error}
            </p>
          )}
        </DialogContent>
      </Dialog>
    </section>
  );
}
