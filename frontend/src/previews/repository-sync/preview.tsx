import {
  Fieldset,
  PageHeader,
  EmptyState,
  Disclosure,
  Select,
  Button,
  Radio,
  Checkbox,
  Input,
  Label,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  Notice,
  FormField,
} from "@pdlc/ui/ui";

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
} from "react";
import { useSearchParams } from "react-router";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  ArrowLeft,
  ArrowLeftRight,
  CirclePause,
  GitBranch,
  Plus,
  RefreshCw,
} from "lucide-react";

type Provider = "gitlab" | "github";
type Mode = "disabled" | "pull" | "push" | "both";
type Scenario = "success" | "auth" | "unavailable" | "conflict" | "partial";
type Connection = {
  provider: Provider;
  instance: string;
  path: string;
  mode: Mode;
  automatic: boolean;
};
type Run = {
  id: number;
  time: string;
  remote: string;
  mode: Mode;
  status: string;
  detail: string;
};
type Entry = {
  id: string;
  repository: string;
  connection: Connection | null;
  runs: Run[];
  running?: boolean;
};
type EditorSnapshot = { draft: Connection; token: string; credential: boolean };

const initial: Connection = {
  provider: "gitlab",
  instance: "https://gitlab.example.org",
  path: "platform/service-api",
  mode: "disabled",
  automatic: true,
};
const modes = [
  {
    value: "disabled",
    title: "Отключено",
    detail: "Подключение сохранено, обмен изменениями остановлен.",
    icon: CirclePause,
  },
  {
    value: "pull",
    title: "Внешний → Forge",
    detail: "Получать ветки и теги из внешнего репозитория.",
    icon: ArrowDownToLine,
  },
  {
    value: "push",
    title: "Forge → внешний",
    detail: "Отправлять ветки и теги во внешний репозиторий.",
    icon: ArrowUpFromLine,
  },
  {
    value: "both",
    title: "В обе стороны",
    detail: "Получать и отправлять изменения.",
    icon: ArrowLeftRight,
  },
] as const;
const initialEntries: Entry[] = [
  {
    id: "service-api",
    repository: "platform / service-api",
    connection: { ...initial, mode: "both" },
    runs: [],
  },
  {
    id: "web-portal",
    repository: "platform / web-portal",
    connection: {
      ...initial,
      provider: "github",
      path: "example-team/web-portal",
      mode: "push",
      automatic: false,
    },
    runs: [],
  },
];

function remoteUrl(connection: Connection): string {
  return `${connection.provider === "github" ? "https://github.com" : connection.instance.replace(/\/$/, "")}/${connection.path}.git`;
}

function PreviewNotice() {
  return (
    <Notice title="Предпросмотр">
      Все данные вымышлены. Действия выполняются в памяти. Используйте только
      вымышленные токены.
    </Notice>
  );
}

function simulatedRun(connection: Connection, scenario: Scenario): Run {
  let status = "Успешно";
  let detail =
    connection.mode === "pull"
      ? "Получены изменения main и тег v1.4.0."
      : connection.mode === "push"
        ? "Отправлены изменения main и тег v1.4.0."
        : "Получена ветка feature/api, отправлены изменения main.";
  if (scenario === "auth") {
    status = "Ошибка доступа";
    detail = "Токен отклонён. Изменения не переданы.";
  }
  if (scenario === "unavailable") {
    status = "Недоступен";
    detail = "Соединение с провайдером не установлено. Повторите запуск позже.";
  }
  if (scenario === "conflict") {
    status = "Конфликт";
    detail =
      "Ветка main имеет расходящуюся историю. Сведите историю через обычный Git/PR и повторите синхронизацию.";
  }
  if (scenario === "partial") {
    status = "Частично выполнено";
    detail =
      "Ветка feature/api синхронизирована. Обновление main отклонено защитой ветки; при повторе уже переданные изменения учитываются.";
  }
  return {
    id: Date.now(),
    time: new Date().toLocaleTimeString("ru-RU", { timeZone: "Europe/Moscow" }),
    remote: remoteUrl(connection),
    mode: connection.mode,
    status,
    detail,
  };
}

export function RepositorySyncPreview() {
  const [params, setParams] = useSearchParams();
  const [entries, setEntries] = useState<Entry[]>(initialEntries);
  const [adding, setAdding] = useState(false);
  const [newRepository, setNewRepository] = useState("worker");
  const [drafts, setDrafts] = useState<Record<string, EditorSnapshot>>({});
  const timers = useRef<Record<string, ReturnType<typeof setTimeout>>>({});
  useEffect(
    () => () => {
      Object.values(timers.current).forEach(clearTimeout);
    },
    [],
  );
  const retainDraft = useCallback((id: string, snapshot: EditorSnapshot) => {
    setDrafts((current) => ({ ...current, [id]: snapshot }));
  }, []);
  const selected = params.get("repository");
  const selectedEntry = entries.find((entry) => entry.id === selected);
  const search = params.get("search") ?? "";
  const provider = params.get("provider") ?? "all";
  const state = params.get("state") ?? "all";
  const configured = entries.filter((entry) => entry.connection);
  const available = ["worker", "docs"].filter(
    (id) => !configured.some((entry) => entry.id === id),
  );
  const visible = configured.filter((entry) => {
    const connection = entry.connection!;
    return (
      `${entry.repository} ${remoteUrl(connection)}`
        .toLocaleLowerCase()
        .includes(search.toLocaleLowerCase()) &&
      (provider === "all" || connection.provider === provider) &&
      (state === "all" ||
        (state === "disabled"
          ? connection.mode === "disabled"
          : connection.mode !== "disabled"))
    );
  });

  function filter(key: string, value: string) {
    const next = new URLSearchParams(params);
    if (value === "all" || !value) next.delete(key);
    else next.set(key, value);
    setParams(next, { replace: true });
  }
  function open(id: string, tab = "connection") {
    const next = new URLSearchParams(params);
    next.set("repository", id);
    next.set("tab", tab);
    setParams(next);
  }
  function back() {
    const next = new URLSearchParams(params);
    next.delete("repository");
    next.delete("tab");
    setParams(next);
  }
  function patch(id: string, changes: Partial<Entry>) {
    setEntries((current) =>
      current.map((entry) =>
        entry.id === id ? { ...entry, ...changes } : entry,
      ),
    );
  }
  function start(id: string, connection: Connection, scenario: Scenario) {
    if (timers.current[id] || connection.mode === "disabled") return;
    patch(id, { running: true });
    timers.current[id] = setTimeout(() => {
      const run = simulatedRun(connection, scenario);
      setEntries((current) =>
        current.map((entry) =>
          entry.id === id
            ? { ...entry, running: false, runs: [run, ...entry.runs] }
            : entry,
        ),
      );
      delete timers.current[id];
    }, 450);
  }
  function add() {
    const id = available.includes(newRepository) ? newRepository : available[0];
    if (!id) return;
    if (!entries.some((entry) => entry.id === id)) {
      setEntries((current) => [
        ...current,
        { id, repository: `platform / ${id}`, connection: null, runs: [] },
      ]);
    }
    setAdding(false);
    open(id);
  }

  return (
    <>
      {!selected && (
        <div className="min-h-screen bg-background text-text-primary">
          <PreviewNotice />
          <header className="flex items-center gap-3 border-b border-border px-4 py-4 sm:px-8">
            <GitBranch aria-hidden className="h-6 w-6 text-accent" />
            <strong>Forge</strong>
            <span className="text-text-muted">/</span>
            <span>Репозитории</span>
          </header>
          <main className="mx-auto max-w-screen-2xl space-y-6 px-4 py-6 sm:px-8">
            <PageHeader
              title="Синхронизации"
              context={
                <>Подключения репозиториев Forge к GitLab и GitHub.com.</>
              }
              actions={
                <>
                  <Button
                    type="button"
                    aria-expanded={adding}
                    onClick={() => setAdding((current) => !current)}
                  >
                    <Plus aria-hidden className="h-4 w-4" />
                    Добавить синхронизацию
                  </Button>
                </>
              }
            />
            {adding && (
              <form
                className="flex flex-wrap items-end gap-3 border-y border-border py-4"
                onSubmit={(event) => {
                  event.preventDefault();
                  add();
                }}
              >
                {available.length ? (
                  <>
                    <div className="min-w-48 flex-1 space-y-2">
                      <Label htmlFor="new-repository">Репозиторий Forge</Label>
                      <Select
                        id="new-repository"
                        className="min-w-0"
                        value={
                          available.includes(newRepository)
                            ? newRepository
                            : available[0]
                        }
                        onValueChange={(nextValue) =>
                          setNewRepository(nextValue)
                        }
                        options={[
                          ...(available.map((id) => ({
                            value: String(id),
                            label: ["platform / ", id].join(""),
                          })) ?? []),
                        ]}
                      />
                    </div>
                    <Button type="submit">Настроить подключение</Button>
                  </>
                ) : (
                  <p className="text-sm">
                    Все демонстрационные репозитории уже подключены.
                  </p>
                )}
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => setAdding(false)}
                >
                  Отмена
                </Button>
              </form>
            )}
            <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_auto_auto]">
              <FormField id="sync-search" label={"Поиск"}>
                {(baseFieldAttrs) => (
                  <Input
                    {...baseFieldAttrs}
                    type="search"
                    placeholder="Репозиторий или внешний адрес"
                    value={search}
                    onChange={(event) => filter("search", event.target.value)}
                  />
                )}
              </FormField>
              <FormField id="provider-filter" label={"Провайдер"}>
                {(baseFieldAttrs) => (
                  <Select
                    {...baseFieldAttrs}
                    className="min-w-0"
                    value={provider}
                    onValueChange={(nextValue) => filter("provider", nextValue)}
                    options={[
                      { value: "all", label: "Все провайдеры" },
                      { value: "gitlab", label: "Свой GitLab" },
                      { value: "github", label: "GitHub.com" },
                    ]}
                  />
                )}
              </FormField>
              <FormField id="state-filter" label={"Состояние"}>
                {(baseFieldAttrs) => (
                  <Select
                    {...baseFieldAttrs}
                    className="min-w-0"
                    value={state}
                    onValueChange={(nextValue) => filter("state", nextValue)}
                    options={[
                      { value: "all", label: "Все подключения" },
                      { value: "active", label: "Включённые" },
                      { value: "disabled", label: "Отключённые" },
                    ]}
                  />
                )}
              </FormField>
            </div>
            <p className="text-sm text-text-muted">
              Показано {visible.length} из {configured.length}
            </p>
            {visible.length === 0 ? (
              <EmptyState
                message="Синхронизации не найдены"
                description="Измените фильтры или добавьте подключение."
              />
            ) : (
              <Table>
                <caption className="sr-only">
                  Список синхронизаций репозиториев
                </caption>
                <TableHeader>
                  <TableRow>
                    <TableHead>Репозиторий Forge</TableHead>
                    <TableHead>Внешний репозиторий</TableHead>
                    <TableHead>Направление</TableHead>
                    <TableHead>Запуск</TableHead>
                    <TableHead>Состояние</TableHead>
                    <TableHead>Последний результат</TableHead>
                    <TableHead>
                      <span className="sr-only">Действия</span>
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {visible.map((entry) => {
                    const connection = entry.connection!;
                    const last = entry.runs.find(
                      (run) => run.remote === remoteUrl(connection),
                    );
                    return (
                      <TableRow key={entry.id}>
                        <TableCell>
                          <Button
                            variant="link"
                            type="button"
                            className="text-left"
                            onClick={() => open(entry.id)}
                          >
                            {entry.repository}
                          </Button>
                        </TableCell>
                        <TableCell>
                          <span className="block font-medium">
                            {connection.provider === "gitlab"
                              ? "Свой GitLab"
                              : "GitHub.com"}
                          </span>
                          <span className="mt-1 block max-w-72 break-all text-xs text-text-secondary">
                            {remoteUrl(connection)}
                          </span>
                        </TableCell>
                        <TableCell>
                          {
                            modes.find((mode) => mode.value === connection.mode)
                              ?.title
                          }
                        </TableCell>
                        <TableCell>
                          {connection.automatic ? "Автоматически" : "Вручную"}
                        </TableCell>
                        <TableCell>
                          {entry.running
                            ? "Выполняется"
                            : connection.mode === "disabled"
                              ? "Отключена"
                              : "Включена"}
                        </TableCell>
                        <TableCell>
                          {last ? (
                            <>
                              <span className="block">{last.status}</span>
                              <span className="text-xs text-text-muted">
                                {last.time}
                              </span>
                            </>
                          ) : (
                            <span className="text-text-muted">
                              Запусков ещё нет
                            </span>
                          )}
                        </TableCell>
                        <TableCell>
                          <Button
                            type="button"
                            size="sm"
                            variant="outline"
                            aria-label={`История ${entry.repository}`}
                            onClick={() => open(entry.id, "history")}
                          >
                            История
                          </Button>
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            )}
          </main>
        </div>
      )}
      {selected && !selectedEntry && (
        <div className="p-8">
          <Notice role="alert" tone="danger" appearance="inline">
            Подключение не найдено.
          </Notice>
          <Button type="button" onClick={back}>
            К списку синхронизаций
          </Button>
        </div>
      )}
      {selectedEntry && (
        <RepositorySyncEditor
          key={selectedEntry.id}
          entry={selectedEntry}
          retained={drafts[selectedEntry.id]}
          onRetain={retainDraft}
          onSave={(connection) => patch(selectedEntry.id, { connection })}
          onStart={(connection, scenario) =>
            start(selectedEntry.id, connection, scenario)
          }
          onBack={back}
        />
      )}
    </>
  );
}

function validate(connection: Connection): string | null {
  if (
    !/^[\w.-]+(?:\/[\w.-]+)+$/.test(connection.path) ||
    connection.path.split("/").some((part) => part === "." || part === "..")
  ) {
    return "Укажите путь репозитория: владелец/репозиторий или группа/подгруппа/репозиторий.";
  }
  if (
    connection.provider === "github" &&
    connection.path.split("/").length !== 2
  )
    return "Для GitHub укажите владельца и репозиторий.";
  if (connection.provider === "gitlab") {
    try {
      const url = new URL(connection.instance);
      if (
        url.protocol !== "https:" ||
        url.username ||
        url.password ||
        url.search ||
        url.hash
      )
        throw new Error();
    } catch {
      return "Укажите HTTPS-адрес GitLab без учётных данных, query-параметров и фрагмента.";
    }
  }
  return null;
}

function RepositorySyncEditor({
  entry,
  retained,
  onRetain,
  onSave,
  onStart,
  onBack,
}: {
  entry: Entry;
  retained?: EditorSnapshot;
  onRetain: (id: string, snapshot: EditorSnapshot) => void;
  onSave: (connection: Connection) => void;
  onStart: (connection: Connection, scenario: Scenario) => void;
  onBack: () => void;
}) {
  const [params, setParams] = useSearchParams();
  const tab = params.get("tab") === "history" ? "history" : "connection";
  const [draft, setDraft] = useState<Connection>(
    retained?.draft ??
      entry.connection ?? { ...initial, path: `platform/${entry.id}` },
  );
  const [saved, setSaved] = useState<Connection | null>(entry.connection);
  const [token, setToken] = useState(retained?.token ?? "");
  const [credential, setCredential] = useState(
    retained?.credential ?? !!entry.connection,
  );
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [scenario, setScenario] = useState<Scenario>("success");
  const [readOnly, setReadOnly] = useState(false);
  const busy = !!entry.running;
  const cleanDraft = saved ?? { ...initial, path: `platform/${entry.id}` };
  const dirty =
    JSON.stringify(draft) !== JSON.stringify(cleanDraft) || token.length > 0;
  const remote = saved ? remoteUrl(saved) : null;
  const runs = entry.runs;
  const lastRun = runs.find((run) => run.remote === remote);

  useEffect(() => {
    onRetain(entry.id, { draft, token, credential });
  }, [entry.id, draft, token, credential, onRetain]);

  useEffect(() => {
    function warn(event: BeforeUnloadEvent) {
      if (dirty) event.preventDefault();
    }
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [dirty]);

  function update<K extends keyof Connection>(key: K, value: Connection[K]) {
    setDraft((current) => ({ ...current, [key]: value }));
    setMessage("");
    setError("");
  }
  function save(event: FormEvent) {
    event.preventDefault();
    if (readOnly || busy) return;
    const normalized = {
      ...draft,
      instance: draft.instance.trim().replace(/\/$/, ""),
      path: draft.path.trim().replace(/\.git$/, ""),
    };
    const problem = validate(normalized);
    if (problem) {
      setError(problem);
      return;
    }
    if (!credential && !token.trim()) {
      setError("Введите демонстрационный токен доступа.");
      return;
    }
    setSaved(normalized);
    setDraft(normalized);
    setCredential(true);
    setToken("");
    setError("");
    onSave(normalized);
    setMessage("Настройки сохранены в предпросмотре.");
  }
  function check() {
    setMessage("");
    const problem = validate(draft);
    if (problem) {
      setError(problem);
      return;
    }
    if (!credential && !token.trim()) {
      setError("Введите демонстрационный токен доступа.");
      return;
    }
    setError("");
    if (scenario === "auth")
      setError("Доступ отклонён. Проверьте токен и права на репозиторий.");
    else if (scenario === "unavailable")
      setError(
        "Провайдер недоступен. Настройки и последний результат сохранены.",
      );
    else
      setMessage(
        draft.mode === "push" || draft.mode === "both"
          ? "Демонстрационная проверка: чтение и запись доступны."
          : "Демонстрационная проверка: чтение доступно.",
      );
  }
  function sync() {
    if (!saved || saved.mode === "disabled" || busy || readOnly || dirty)
      return;
    setError("");
    setMessage("");
    onStart(saved, scenario);
  }

  return (
    <div className="min-h-screen bg-background text-text-primary">
      <PreviewNotice />
      <header className="flex flex-wrap items-center justify-between gap-3 border-b border-border px-4 py-4 sm:px-8">
        <div className="flex items-center gap-3">
          <GitBranch aria-hidden className="h-6 w-6 text-accent" />
          <strong>Forge</strong>
          <span className="text-text-muted">/</span>
          <span>{entry.repository}</span>
        </div>
        <span className="text-sm text-text-secondary">
          Настройки репозитория
        </span>
      </header>
      <main className="mx-auto max-w-6xl space-y-3 px-4 py-4 sm:px-8">
        <PageHeader
          title="Внешний репозиторий"
          leading={
            <>
              <Button
                type="button"
                size="icon"
                variant="ghost"
                aria-label="Все синхронизации"
                title="Все синхронизации"
                onClick={onBack}
              >
                <ArrowLeft aria-hidden className="h-4 w-4" />
              </Button>
            </>
          }
          actions={
            <>
              <Button
                type="button"
                onClick={sync}
                disabled={
                  !saved ||
                  saved.mode === "disabled" ||
                  dirty ||
                  busy ||
                  readOnly
                }
              >
                <RefreshCw
                  aria-hidden
                  className={`h-4 w-4 ${busy ? "animate-spin" : ""}`}
                />
                {busy ? "Синхронизация…" : "Синхронизировать сейчас"}
              </Button>
            </>
          }
        />
        <section
          aria-label="Состояние подключения"
          className="flex flex-wrap items-center gap-x-4 gap-y-1 border-b border-border pb-3 text-xs"
        >
          <strong className="shrink-0">
            {busy
              ? "Синхронизация выполняется"
              : !saved
                ? "Подключение не настроено"
                : saved.mode === "disabled"
                  ? "Синхронизация отключена"
                  : modes.find((mode) => mode.value === saved.mode)?.title}
          </strong>
          {remote && (
            <span className="min-w-0 break-all text-text-secondary">
              {remote}
            </span>
          )}
          <span className="text-text-secondary sm:ml-auto">
            {lastRun
              ? `${lastRun.time} · ${lastRun.status}`
              : "Запусков ещё нет"}
          </span>
        </section>
        {readOnly && (
          <p className="border-l-2 border-border-strong pl-3 text-sm">
            Доступ только для чтения. Изменение подключения и запуск
            синхронизации недоступны.
          </p>
        )}
        {error && (
          <Notice role="alert" tone="danger" appearance="inline">
            {error}
          </Notice>
        )}
        {!message &&
          !error &&
          !busy &&
          lastRun &&
          lastRun.status !== "Успешно" &&
          tab === "connection" && (
            <Notice role="alert" tone="danger" appearance="inline">
              {lastRun.detail}
            </Notice>
          )}
        <Tabs
          activationMode="manual"
          value={tab}
          onValueChange={(value) => {
            const next = new URLSearchParams(params);
            next.set("tab", value);
            setParams(next);
          }}
        >
          <TabsList aria-label="Внешний репозиторий">
            <TabsTrigger value="connection">Подключение</TabsTrigger>
            <TabsTrigger value="history">
              История запусков{runs.length ? ` (${runs.length})` : ""}
            </TabsTrigger>
          </TabsList>
          <TabsContent
            value="connection"
            forceMount
            hidden={tab !== "connection"}
          >
            <form
              onSubmit={save}
              className="mt-3 space-y-3"
              aria-label="Подключение внешнего репозитория"
            >
              <Fieldset disabled={readOnly || busy} className="space-y-3">
                <div className="grid gap-3 md:grid-cols-2">
                  <FormField id="provider" label={"Провайдер"}>
                    {(baseFieldAttrs) => (
                      <Select
                        {...baseFieldAttrs}
                        className="min-w-0"
                        value={draft.provider}
                        onValueChange={(nextValue) => {
                          update("provider", nextValue as Provider);
                          setCredential(false);
                          setToken("");
                        }}
                        options={[
                          { value: "gitlab", label: "Свой GitLab" },
                          { value: "github", label: "GitHub.com" },
                        ]}
                      />
                    )}
                  </FormField>
                  {draft.provider === "gitlab" ? (
                    <FormField id="instance" label={"Адрес инстанса GitLab"}>
                      {(baseFieldAttrs) => (
                        <Input
                          {...baseFieldAttrs}
                          type="url"
                          required
                          value={draft.instance}
                          onChange={(event) => {
                            update("instance", event.target.value);
                            setCredential(false);
                          }}
                          placeholder="https://gitlab.example.org"
                        />
                      )}
                    </FormField>
                  ) : (
                    <FormField id="github-host" label={"Адрес GitHub"}>
                      {(baseFieldAttrs) => (
                        <Input
                          {...baseFieldAttrs}
                          value="https://github.com"
                          readOnly
                        />
                      )}
                    </FormField>
                  )}
                  <div className="space-y-2">
                    <Label htmlFor="repository">
                      {draft.provider === "gitlab"
                        ? "Группа / репозиторий"
                        : "Владелец / репозиторий"}
                    </Label>
                    <Input
                      id="repository"
                      required
                      value={draft.path}
                      onChange={(event) => update("path", event.target.value)}
                      placeholder="team/service-api"
                    />
                    <p className="text-xs text-text-muted">
                      {draft.provider === "gitlab"
                        ? "Подгруппы поддерживаются: team/backend/service-api."
                        : "Личный репозиторий или репозиторий организации."}
                    </p>
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="token">Токен доступа</Label>
                    <Input
                      id="token"
                      type="password"
                      autoComplete="off"
                      value={token}
                      onChange={(event) => {
                        setToken(event.target.value);
                        setMessage("");
                        setError("");
                      }}
                      placeholder={
                        credential
                          ? "Токен сохранён · введите новый для замены"
                          : "Вымышленный токен для предпросмотра"
                      }
                    />
                    <p className="text-xs text-text-muted">
                      {credential
                        ? "Текущее значение не отображается."
                        : "Права должны соответствовать выбранному направлению."}
                    </p>
                  </div>
                </div>
                <Fieldset legend={"Направление синхронизации"}>
                  <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
                    {modes.map((mode) => (
                      <Radio
                        key={mode.value}
                        appearance="card"
                        name="mode"
                        value={mode.value}
                        checked={draft.mode === mode.value}
                        onCheckedChange={() => update("mode", mode.value)}
                      >
                        <span className="min-w-0">
                          <strong className="flex items-center gap-2 text-sm">
                            <mode.icon
                              aria-hidden
                              className="h-3.5 w-3.5 shrink-0 text-text-secondary"
                            />
                            {mode.title}
                          </strong>
                          <span className="mt-1 block text-xs leading-4 text-text-secondary">
                            {mode.detail}
                          </span>
                        </span>
                      </Radio>
                    ))}
                  </div>
                </Fieldset>
                <Fieldset
                  disabled={draft.mode === "disabled"}
                  className="grid grid-cols-2 items-center gap-x-3 sm:flex sm:flex-wrap sm:gap-x-6"
                  legend={"Способ запуска"}
                  legendVisuallyHidden
                >
                  <span aria-hidden className="col-span-2 text-sm font-medium">
                    Способ запуска
                  </span>
                  <Radio
                    name={`launch-${entry.id}`}
                    checked={!draft.automatic}
                    onCheckedChange={() => update("automatic", false)}
                  >
                    Вручную
                  </Radio>
                  <Radio
                    name={`launch-${entry.id}`}
                    checked={draft.automatic}
                    onCheckedChange={() => update("automatic", true)}
                  >
                    Автоматически
                  </Radio>
                </Fieldset>
                <p className="text-xs text-text-secondary">
                  {draft.mode === "disabled"
                    ? "Отключение сохраняет подключение и историю. Ручной и автоматический запуск остановлены."
                    : !draft.automatic
                      ? "Синхронизация запускается только вручную."
                      : draft.mode === "pull"
                        ? "Внешний репозиторий проверяется периодически."
                        : draft.mode === "push"
                          ? "Изменения Forge отправляются после push."
                          : "Изменения Forge отправляются после push; внешний репозиторий проверяется периодически."}
                </p>
                <p className="text-xs text-text-secondary">
                  Расхождение истории и несовпадающие теги требуют решения в
                  Git. Синхронизация не перезаписывает историю принудительно и
                  не удаляет ветки автоматически.
                </p>
              </Fieldset>
              <div className="bottom-0 flex flex-wrap items-center gap-3 border-t border-border bg-background py-3 sm:sticky">
                <Button
                  type="submit"
                  disabled={readOnly || busy || (!dirty && !!saved)}
                >
                  Сохранить подключение
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  disabled={readOnly || busy}
                  onClick={check}
                >
                  Проверить доступ
                </Button>
                {dirty && (
                  <>
                    <Button
                      type="button"
                      variant="ghost"
                      disabled={busy}
                      onClick={() => {
                        setDraft(cleanDraft);
                        setCredential(!!saved);
                        setToken("");
                        setError("");
                        setMessage("");
                      }}
                    >
                      Отменить изменения
                    </Button>
                    <span className="text-xs text-text-muted">
                      Есть несохранённые изменения
                    </span>
                  </>
                )}
              </div>
            </form>
          </TabsContent>
          <TabsContent value="history">
            <div className="mt-5 space-y-4">
              {runs.length === 0 ? (
                <EmptyState
                  message="Запусков ещё нет"
                  description="Сохраните подключение и выберите активное направление."
                />
              ) : (
                runs.map((run) => (
                  <article key={run.id} className="border-b border-border pb-4">
                    <div className="flex flex-wrap justify-between gap-2">
                      <strong className="text-sm">{run.status}</strong>
                      <span className="text-xs text-text-muted">
                        {run.time} ·{" "}
                        {modes.find((mode) => mode.value === run.mode)?.title}
                      </span>
                    </div>
                    <p className="mt-2 break-all text-xs text-text-muted">
                      {run.remote}
                    </p>
                    <p className="mt-2 text-sm text-text-secondary">
                      {run.detail}
                    </p>
                  </article>
                ))
              )}
            </div>
          </TabsContent>
        </Tabs>
        {message && (
          <Notice role="status" tone="neutral" appearance="inline">
            {message}
          </Notice>
        )}
        <Disclosure className="mt-4" summary={<>Сценарии предпросмотра</>}>
          <div className="mt-4 flex flex-wrap items-end gap-5">
            <FormField id="scenario" label={"Результат следующего запуска"}>
              {(baseFieldAttrs) => (
                <Select
                  {...baseFieldAttrs}
                  className="min-w-0"
                  disabled={busy}
                  value={scenario}
                  onValueChange={(nextValue) =>
                    setScenario(nextValue as Scenario)
                  }
                  options={[
                    { value: "success", label: "Успех" },
                    { value: "auth", label: "Ошибка доступа" },
                    { value: "unavailable", label: "Провайдер недоступен" },
                    { value: "conflict", label: "Расхождение истории" },
                    { value: "partial", label: "Частичный результат" },
                  ]}
                />
              )}
            </FormField>
            <Checkbox
              disabled={busy}
              checked={readOnly}
              onCheckedChange={(checked) => setReadOnly(checked === true)}
            >
              Только чтение
            </Checkbox>
          </div>
        </Disclosure>
      </main>
    </div>
  );
}
