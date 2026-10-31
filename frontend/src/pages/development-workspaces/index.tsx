import { useRef, useState, type FormEvent } from "react";
import { useSearchParams } from "react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Card,
  CardContent,
  CardTitle,
  ErrorState,
  Input,
  Label,
  LoadingState,
  Notice,
  SearchableSelect,
  Select,
  NamespaceLink as Link,
  usePlatformServices,
} from "@pdlc/ui/ui";
import { ApiError } from "@/api/client";
import {
  developmentWorkspaces as service,
  developmentWorkspaceKeys as keys,
  type WorkspaceCommand,
  type OperationRequest,
  type RegisterWorkspace,
} from "@/api/development-workspaces";
import { useNamespaceRef } from "@/widgets/namespace-context";

const stateNames: Record<string, string> = {
  unprepared: "Не подготовлено",
  prepared: "Подготовлено",
  degraded: "Требует внимания",
  unknown: "Неизвестно",
  stopped: "Выключено",
  starting: "Включается",
  running: "Работает",
  stopping: "Выключается",
  queued: "В очереди",
  succeeded: "Успешно",
  failed: "Ошибка",
};
const stateName = (value: string) => stateNames[value] ?? value;
const message = (error: unknown) =>
  error instanceof ApiError && error.kind === "network"
    ? "Сервер недоступен. Результат отправленной операции нужно прочитать по исходному ключу."
    : error instanceof Error
      ? error.message
      : "Действие не выполнено";
const object = (value: unknown): Record<string, unknown> =>
  value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
const records = (value: unknown) =>
  Array.isArray(value) ? value.map(object) : [];
const active = (state?: string) =>
  !!state && ["queued", "running", "unknown"].includes(state);

export function DevelopmentWorkspacesPage() {
  const { ref } = useNamespaceRef();
  const [params, setParams] = useSearchParams();
  const project = params.get("workspace_configuration") ?? "";
  const selected = params.get("development_workspace") ?? "";
  const offset = Math.max(
    0,
    Number(params.get("development_workspace_offset")) || 0,
  );
  const [search, setSearch] = useState("");
  const [configOffset, setConfigOffset] = useState(0);
  const configurations = useQuery({
    queryKey: keys.configurations(ref, search, configOffset),
    enabled: !!ref,
    queryFn: ({ signal }) =>
      service.configurations(ref!, search, configOffset, signal),
  });
  const configuration = useQuery({
    queryKey: keys.configuration(project),
    enabled: !!project,
    queryFn: ({ signal }) => service.configuration(project, signal),
    retry: false,
  });
  const repositoryId = configuration.data?.repository_id ?? "";
  const repository = useQuery({
    queryKey: keys.repository(repositoryId),
    enabled: !!repositoryId,
    queryFn: ({ signal }) => service.repository(repositoryId, signal),
    retry: false,
  });
  const correctProject =
    !!ref &&
    repository.data?.namespace?.namespace_id === ref.namespace_id &&
    repository.data?.namespace?.registry_instance_id ===
      ref.registry_instance_id;
  const list = useQuery({
    queryKey: keys.list(project, offset),
    enabled: !!project && correctProject,
    queryFn: ({ signal }) => service.list(project, offset, signal),
    retry: false,
    refetchInterval: 10000,
  });
  function select(key: string, value: string) {
    setParams((current) => {
      const next = new URLSearchParams(current);
      if (value) next.set(key, value);
      else next.delete(key);
      if (key === "workspace_configuration") {
        next.delete("development_workspace");
        next.delete("development_workspace_offset");
      }
      return next;
    });
  }
  return (
    <section className="space-y-5" aria-label="Рабочие окружения">
      <div>
        <h2 className="text-lg font-semibold">Рабочие окружения</h2>
        <p className="text-text-muted">
          Исходники, инструменты, сервисы и данные сохраняются между рабочими
          сессиями.
        </p>
      </div>
      <SearchableSelect
        label="Конфигурация проекта"
        placeholder="Выберите конфигурацию"
        value={project}
        selectedLabel={
          configurations.data?.items.find((item) => item.id === project)?.name
        }
        options={(configurations.data?.items ?? []).map((item) => ({
          value: item.id,
          label: `${item.repository_name ?? "Репозиторий"} / ${item.name}`,
        }))}
        search={search}
        onSearch={(value) => {
          setSearch(value);
          setConfigOffset(0);
        }}
        offset={configOffset}
        onPage={setConfigOffset}
        pageSize={20}
        total={configurations.data?.total ?? 0}
        loading={configurations.isFetching}
        error={
          configurations.isError ? message(configurations.error) : undefined
        }
        onRetry={() => void configurations.refetch()}
        onChange={(value) => select("workspace_configuration", value)}
      />
      {project && (configuration.isError || repository.isError) && (
        <ErrorState message="Конфигурация проекта недоступна." />
      )}
      {project && repository.data && !correctProject && (
        <ErrorState message="Выбранная конфигурация принадлежит другому проекту. Выберите конфигурацию текущего проекта." />
      )}
      {project && correctProject && (
        <Registration
          key={project}
          project={project}
          onRegistered={(id) => select("development_workspace", id)}
        />
      )}
      {list.isPending && project && correctProject && (
        <LoadingState message="Загружаем окружения…" />
      )}
      {list.isError && <ErrorState message={message(list.error)} />}
      {list.data && (
        <>
          <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
            {list.data.items.map((workspace) => (
              <Card key={workspace.id}>
                <CardContent className="space-y-3">
                  <CardTitle>{workspace.name}</CardTitle>
                  <p>
                    {stateName(workspace.runtime)} ·{" "}
                    {stateName(workspace.preparation)}
                  </p>
                  <p className="text-sm text-text-muted">
                    Профиль{" "}
                    {String(
                      object(workspace.profile).name ?? workspace.profile_id,
                    )}
                  </p>
                  <Button
                    variant="outline"
                    onClick={() =>
                      select("development_workspace", workspace.id)
                    }
                  >
                    Открыть окружение
                  </Button>
                </CardContent>
              </Card>
            ))}
          </div>
          {!list.data.total && (
            <Notice tone="neutral" appearance="inline">
              Рабочих окружений пока нет. Подключите существующее или создайте
              из опубликованного профиля.
            </Notice>
          )}
          {list.data.total > 20 && (
            <nav aria-label="Страницы окружений" className="flex gap-2">
              <Button
                disabled={!offset}
                onClick={() =>
                  select(
                    "development_workspace_offset",
                    String(Math.max(0, offset - 20)),
                  )
                }
              >
                Назад
              </Button>
              <span>
                {offset + 1}–{offset + list.data.items.length} из{" "}
                {list.data.total}
              </span>
              <Button
                disabled={offset + 20 >= list.data.total}
                onClick={() =>
                  select("development_workspace_offset", String(offset + 20))
                }
              >
                Далее
              </Button>
            </nav>
          )}
        </>
      )}
      {selected && correctProject && (
        <WorkspaceCard key={selected} id={selected} configuration={project} />
      )}
    </section>
  );
}

function Registration({
  project,
  onRegistered,
}: {
  project: string;
  onRegistered: (id: string) => void;
}) {
  const cache = useQueryClient();
  const targets = useQuery({
    queryKey: keys.targets(project),
    queryFn: ({ signal }) => service.targets(project, signal),
    retry: false,
  });
  const [target, setTarget] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const pending = useRef<RegisterWorkspace | null>(null);
  const [pendingRequest, setPendingRequest] =
    useState<RegisterWorkspace | null>(null);
  const choice = targets.data?.items.find((item) => item.id === target);
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (busy || !choice || !name.trim()) return;
    setBusy(true);
    setError("");
    const request = pending.current ?? {
      id: crypto.randomUUID(),
      operation_id: crypto.randomUUID(),
      catalog_target_id: target,
      name: name.trim(),
      create: choice.kind === "template",
    };
    pending.current = request;
    setPendingRequest(request);
    try {
      await service.register(project, request);
      pending.current = null;
      setPendingRequest(null);
      await cache.invalidateQueries({
        queryKey: ["development-workspaces", project],
      });
      onRegistered(request.id);
    } catch (error) {
      setError(message(error));
      if (
        error instanceof ApiError &&
        [400, 403, 404, 409, 422].includes(error.status)
      ) {
        pending.current = null;
        setPendingRequest(null);
      }
    } finally {
      setBusy(false);
    }
  }
  return (
    <form onSubmit={(event) => void submit(event)} className="space-y-3">
      {targets.isError && (
        <Notice role="alert" tone="warning" appearance="inline">
          Профили недоступны: {message(targets.error)}
        </Notice>
      )}
      {targets.data && (
        <>
          <Label htmlFor="workspace-target">Профиль и место размещения</Label>
          <Select
            id="workspace-target"
            value={target}
            disabled={busy || !!pendingRequest}
            onValueChange={setTarget}
            options={targets.data.items.map((item) => ({
              value: item.id,
              label: `${item.name} · ${item.kind === "existing" ? "подключить" : "создать"}`,
            }))}
          />
          <Label htmlFor="workspace-name">Название окружения</Label>
          <Input
            id="workspace-name"
            value={name}
            maxLength={128}
            disabled={busy || !!pendingRequest}
            onChange={(event) => setName(event.target.value)}
          />
          <Button
            type="submit"
            disabled={
              busy || !choice || !name.trim() || !targets.data.can_register
            }
            title={targets.data.denied_reason ?? undefined}
          >
            {busy
              ? "Отправляем…"
              : choice?.kind === "existing"
                ? "Подключить окружение"
                : "Создать окружение"}
          </Button>
          {targets.data.denied_reason && (
            <p className="text-sm text-text-muted">
              {targets.data.denied_reason}
            </p>
          )}
          {!targets.data.items.length && (
            <p>Оператор ещё не опубликовал профили для этой конфигурации.</p>
          )}
        </>
      )}
      {error && (
        <Notice role="alert" tone="warning" appearance="inline">
          {error}
          {pendingRequest && (
            <p>
              Исходная операция: {pendingRequest.operation_id}. Повторная
              отправка сохранит этот ключ.
            </p>
          )}
        </Notice>
      )}
    </form>
  );
}

function WorkspaceCard({
  id,
  configuration,
}: {
  id: string;
  configuration: string;
}) {
  const { services } = usePlatformServices();
  const cache = useQueryClient();
  const detail = useQuery({
    queryKey: keys.detail(id),
    queryFn: ({ signal }) => service.detail(id, signal),
    retry: false,
    refetchInterval: (query) =>
      active(query.state.data?.latest_operation?.state) ? 3000 : 15000,
  });
  const history = useQuery({
    queryKey: keys.operations(id),
    queryFn: ({ signal }) => service.operations(id, signal),
    retry: false,
    refetchInterval: 10000,
  });
  const [task, setTask] = useState("");
  const [recipe, setRecipe] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const pending = useRef<OperationRequest | null>(null);
  const [pendingRequest, setPendingRequest] = useState<OperationRequest | null>(
    null,
  );
  if (detail.isPending) return <LoadingState message="Загружаем карточку…" />;
  if (detail.isError) return <ErrorState message={message(detail.error)} />;
  if (detail.data.workspace.project_id !== configuration)
    return <ErrorState message="Окружение принадлежит другой конфигурации." />;
  const data = detail.data,
    workspace = data.workspace,
    session = data.session,
    operation = data.latest_operation;
  const sources = records(workspace.sources);
  const trackerOrigin = services.find(
    (service) => service.key === "task-tracker",
  )?.ui_url;
  const commit = sources.find(
    (source) => source.repository_id === workspace.repository_id,
  )?.commit;
  const checks = records(object(workspace.profile).checks).filter(
    (check) =>
      check.executor !== "interactive-browser" && check.key !== "build",
  );
  const capability = (action: string) =>
    data.actions.find((item) => item.action === action);
  const disabled = (action: string) =>
    busy || !!pendingRequest || !capability(action)?.allowed;
  async function submit(command: WorkspaceCommand) {
    if (busy) return;
    setBusy(true);
    setError("");
    const request = pending.current ?? {
      operation_id: crypto.randomUUID(),
      expected_version: workspace.version,
      command,
    };
    pending.current = request;
    setPendingRequest(request);
    try {
      await service.submit(id, request);
      pending.current = null;
      setPendingRequest(null);
      await refresh();
    } catch (error) {
      setError(message(error));
      if (
        error instanceof ApiError &&
        [400, 403, 404, 409, 422].includes(error.status)
      ) {
        pending.current = null;
        setPendingRequest(null);
      }
    } finally {
      setBusy(false);
    }
  }
  async function refresh() {
    await Promise.all([
      cache.invalidateQueries({ queryKey: keys.detail(id) }),
      cache.invalidateQueries({ queryKey: keys.operations(id) }),
      cache.invalidateQueries({
        queryKey: ["development-workspaces", workspace.project_id],
      }),
    ]);
  }
  async function readPending() {
    if (!pending.current) return;
    setBusy(true);
    try {
      await service.operation(id, pending.current.operation_id);
      pending.current = null;
      setPendingRequest(null);
      setError("");
      await refresh();
    } catch (error) {
      setError(message(error));
    } finally {
      setBusy(false);
    }
  }
  async function reconcile() {
    if (!operation || busy) return;
    setBusy(true);
    setError("");
    try {
      await service.reconcile(id, operation.id);
      await refresh();
    } catch (error) {
      setError(message(error));
    } finally {
      setBusy(false);
    }
  }
  const button = (action: string, label: string, command: WorkspaceCommand) => (
    <span className="inline-flex flex-col gap-1">
      <Button
        disabled={disabled(action)}
        title={capability(action)?.reason ?? undefined}
        onClick={() => void submit(command)}
      >
        {label}
      </Button>
      {capability(action)?.reason && (
        <span className="max-w-xs text-xs text-text-muted">
          {capability(action)?.reason}
        </span>
      )}
    </span>
  );
  return (
    <Card>
      <CardContent className="space-y-5">
        <CardTitle>{workspace.name}</CardTitle>
        <p className="break-words text-sm text-text-muted">
          Владелец окружения: {workspace.owner_name ?? workspace.created_by}
        </p>
        {workspace.public_origin && (
          <a
            href={workspace.public_origin}
            target="_blank"
            rel="noreferrer"
            className="text-accent underline"
          >
            Открыть приложение
          </a>
        )}
        <dl className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <div>
            <dt>Подготовленность</dt>
            <dd>{stateName(workspace.preparation)}</dd>
          </div>
          <div>
            <dt>Окружение</dt>
            <dd>{stateName(workspace.runtime)}</dd>
          </div>
          <div>
            <dt>Занятость</dt>
            <dd>
              {active(operation?.state)
                ? "Выполняется операция"
                : session
                  ? "В работе"
                  : "Свободно"}
            </dd>
          </div>
          <div>
            <dt>Последнее наблюдение</dt>
            <dd>
              {workspace.observed_at
                ? new Date(workspace.observed_at).toLocaleString("ru-RU")
                : "Ещё не подтверждено"}
            </dd>
          </div>
        </dl>
        {(workspace.runtime === "unknown" ||
          detail.dataUpdatedAt - Date.parse(workspace.observed_at ?? "") >
            60000) && (
          <Notice tone="warning" appearance="inline">
            Состояние требует свежего наблюдения. Успешный запуск процессов не
            подтверждает готовность разработки или прохождение тестов.
          </Notice>
        )}
        {error && (
          <Notice role="alert" tone="warning" appearance="inline">
            {error}
          </Notice>
        )}
        {pendingRequest && (
          <Notice tone="warning" appearance="panel">
            Ответ команды ещё не подтверждён. Операция{" "}
            {pendingRequest.operation_id}.
            <Button disabled={busy} onClick={() => void readPending()}>
              Прочитать исходную операцию
            </Button>
            <Button
              disabled={busy}
              onClick={() => void submit(pendingRequest!.command)}
            >
              Повторить исходный запрос
            </Button>
          </Notice>
        )}
        <div className="flex flex-wrap gap-3">
          {button("observe", "Обновить состояние", { action: "observe" })}
          {button("start", "Включить", { action: "start" })}
          {button("stop", "Выключить", { action: "stop" })}
        </div>
        <section className="space-y-3">
          <h3 className="font-semibold">Рабочая сессия</h3>
          {session ? (
            <>
              <p className="break-words">
                Владелец: {session.actor_name ?? session.actor_id} · задача{" "}
                {trackerOrigin ? (
                  <a
                    className="text-accent underline"
                    href={`${trackerOrigin.replace(/\/$/, "")}/issues/${encodeURIComponent(session.task_ref)}`}
                  >
                    {session.task_ref}
                  </a>
                ) : (
                  session.task_ref
                )}{" "}
                · с {new Date(session.started_at).toLocaleString("ru-RU")}
              </p>
              <div className="flex flex-wrap gap-3">
                {button("end_session", "Закончить работу", {
                  action: "end_session",
                  session_id: session.id,
                })}
                {button("recover_session", "Восстановить занятую сессию", {
                  action: "recover_session",
                  session_id: session.id,
                })}
              </div>
            </>
          ) : (
            <>
              <Label htmlFor={`task-${id}`}>Задача Tracker</Label>
              <Input
                id={`task-${id}`}
                value={task}
                placeholder="Например, R1-7"
                onChange={(event) => setTask(event.target.value)}
              />
              <Button
                disabled={disabled("begin_session") || !task.trim()}
                title={capability("begin_session")?.reason ?? undefined}
                onClick={() =>
                  void submit({ action: "begin_session", task: task.trim() })
                }
              >
                Начать работу
              </Button>
              {capability("begin_session")?.reason && (
                <p>{capability("begin_session")?.reason}</p>
              )}
            </>
          )}
        </section>
        <section className="space-y-3">
          <h3 className="font-semibold">Исходники и инструменты</h3>
          <p>
            Профиль{" "}
            {String(object(workspace.profile).name ?? workspace.profile_id)} ·{" "}
            {workspace.profile_hash}
          </p>
          {sources.map((source) => (
            <p className="break-all" key={String(source.key)}>
              {String(source.key)} · {String(source.commit)}
            </p>
          ))}
          <p className="text-sm text-text-muted">
            Локальные checkout подготавливаются командой cicd-cli dev-workspace
            prepare. Выбирайте полный commit каждого репозитория;
            незакоммиченные файлы остаются локально.
          </p>
          {button("prepare", "Проверить подготовленность", {
            action: "prepare",
            sources: workspace.sources as Extract<
              WorkspaceCommand,
              { action: "prepare" }
            >["sources"],
            local: workspace.local_preparation as Extract<
              WorkspaceCommand,
              { action: "prepare" }
            >["local"],
          })}
          <a
            className="text-accent underline"
            href="https://github.com/FerrPOINT/services-base/blob/main/docs/platform/WORKSPACES.md"
            target="_blank"
            rel="noreferrer"
          >
            Документация workspace
          </a>
        </section>
        <section className="space-y-3">
          <h3 className="font-semibold">Проверки и сборка</h3>
          <Label htmlFor={`recipe-${id}`}>Опубликованный рецепт</Label>
          <Select
            id={`recipe-${id}`}
            value={recipe}
            onValueChange={setRecipe}
            options={checks.map((check) => ({
              value: String(check.key),
              label: String(check.key),
            }))}
          />
          <div className="flex flex-wrap gap-3">
            <Button
              disabled={
                disabled("check") || !recipe || typeof commit !== "string"
              }
              title={capability("check")?.reason ?? undefined}
              onClick={() =>
                typeof commit === "string" &&
                void submit({ action: "check", recipe, source_commit: commit })
              }
            >
              Запустить проверку
            </Button>
            <Button
              disabled={disabled("build") || typeof commit !== "string"}
              title={capability("build")?.reason ?? undefined}
              onClick={() =>
                typeof commit === "string" &&
                void submit({ action: "build", source_commit: commit })
              }
            >
              Собрать приложение
            </Button>
          </div>
          {capability("check")?.reason && <p>{capability("check")?.reason}</p>}
        </section>
        {operation && (
          <section className="space-y-3">
            <h3 className="font-semibold">Текущая операция</h3>
            <p>
              {String(object(operation.request).action ?? "")} ·{" "}
              {stateName(operation.state)} · {operation.id}
            </p>
            {operation.state === "unknown" && (
              <>
                <Notice tone="warning" appearance="inline">
                  Исход операции неизвестен. Новое изменение окружения запрещено
                  до сверки.
                </Notice>
                <Button
                  disabled={busy || !capability("reconcile")?.allowed}
                  title={capability("reconcile")?.reason ?? undefined}
                  onClick={() => void reconcile()}
                >
                  Восстановить исходную операцию
                </Button>
              </>
            )}
            {operation.pipeline_id && (
              <Link
                to={`/pipelines/${operation.pipeline_id}`}
                className="text-accent underline"
              >
                Открыть результат и логи
              </Link>
            )}
          </section>
        )}
        <section className="space-y-2">
          <h3 className="font-semibold">История операций</h3>
          {history.isError && (
            <Notice tone="warning" appearance="inline">
              История недоступна: {message(history.error)}
            </Notice>
          )}
          {history.data?.map((item) => (
            <p className="break-all" key={item.id}>
              {new Date(item.created_at).toLocaleString("ru-RU")} ·{" "}
              {String(object(item.request).action ?? "")} ·{" "}
              {stateName(item.state)}
              {item.pipeline_id && (
                <>
                  {" "}
                  ·{" "}
                  <Link
                    className="text-accent underline"
                    to={`/pipelines/${item.pipeline_id}`}
                  >
                    Результат и логи
                  </Link>
                </>
              )}
            </p>
          ))}
        </section>
      </CardContent>
    </Card>
  );
}
