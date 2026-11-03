import { useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ActionGroup, Button, Checkbox, ErrorState, Fieldset, FormField, Input,
  LoadingState, Notice, SectionHeader, UnsavedChangesGuard,
} from "@pdlc/ui/ui";
import { ApiError, apiRetry } from "@/api/client";
import { repositorySync, syncScheduleKey, type SaveSyncSchedule, type SyncSchedule } from "@/api/repository-sync";
import { useAuth } from "@/shared/auth/auth-provider";

export function SyncSchedulePanel() {
  const { session } = useAuth();
  const key = syncScheduleKey(session?.subject);
  const cache = useQueryClient();
  const policy = useQuery({ queryKey: key, queryFn: ({ signal }) => repositorySync.schedule(signal),
    enabled: !!session?.subject, retry: apiRetry, refetchOnWindowFocus: false });
  return (
    <section aria-labelledby="sync-schedule-heading">
      <SectionHeader headingId="sync-schedule-heading" title="Синхронизация репозиториев" />
      {!session?.subject && <Notice tone="warning">Для управления расписанием войдите как администратор Forge.</Notice>}
      {session?.subject && policy.isPending && <LoadingState scope="local" message="Загружаем расписание…" />}
      {policy.isError && !policy.data && <ErrorState scope="local" message="Не удалось загрузить расписание. Нужен доступ администратора Forge."
        onRetry={() => void policy.refetch()} retryDisabled={policy.isFetching} />}
      {policy.data && <SyncScheduleEditor key={session?.subject} initial={policy.data}
        saved={(value) => cache.setQueryData(key, value)}
        reload={async () => { const result = await policy.refetch({ throwOnError: true }); return result.data!; }} />}
    </section>
  );
}

function SyncScheduleEditor({ initial, saved, reload }: {
  initial: SyncSchedule; saved: (value: SyncSchedule) => void; reload: () => Promise<SyncSchedule>;
}) {
  const [baseline, setBaseline] = useState(initial);
  const [seconds, setSeconds] = useState(String(initial.settings.interval_seconds));
  const [onChange, setOnChange] = useState(initial.settings.on_change);
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  // An uncertain response retains exactly the submitted command and key.
  const [intent, setIntent] = useState<{ operation: string; payload: SaveSyncSchedule } | null>(null);
  const uncertain = intent !== null;
  const dirty = Number(seconds) !== baseline.settings.interval_seconds || onChange !== baseline.settings.on_change;
  const valid = seconds.trim() !== "" && Number.isSafeInteger(Number(seconds)) && Number(seconds) >= 1 && Number(seconds) <= 86400;
  function accept(value: SyncSchedule) {
    setIntent(null);
    setBaseline(value); setSeconds(String(value.settings.interval_seconds)); setOnChange(value.settings.on_change);
    saved(value);
  }
  async function save(event: FormEvent) {
    event.preventDefault();
    if (pending || (!intent && (!dirty || !valid))) return;
    const command = intent ?? { operation: crypto.randomUUID(), payload: {
      expected_version: baseline.version, settings: { interval_seconds: Number(seconds), on_change: onChange },
    }};
    setIntent(command);
    setPending(true); setError(""); setMessage("");
    try {
      const value = await repositorySync.saveSchedule(command.payload, command.operation);
      const superseded = value.version > command.payload.expected_version + 1;
      accept(value); setMessage(superseded ? "Запрос подтверждён. Показана более новая версия расписания." : "Расписание сохранено и применяется без перезапуска.");
    } catch (failure) {
      const definitive = failure instanceof ApiError && failure.kind === "api" && failure.status >= 400 && failure.status < 500;
      if (definitive) setIntent(null);
      setError(definitive
        ? failure.status === 409 ? "Расписание изменилось. Загрузите актуальные настройки перед сохранением." : failure.message
        : "Результат сохранения не подтверждён. Проверьте его тем же запросом.");
    } finally { setPending(false); }
  }
  async function refresh() {
    if (pending) return;
    setPending(true); setError(""); setMessage("");
    try { accept(await reload()); } catch { setError("Не удалось обновить расписание. Правки сохранены в форме."); }
    finally { setPending(false); }
  }
  return <>
    <UnsavedChangesGuard when={dirty || pending || uncertain} />
    {!baseline.worker_enabled && <Notice appearance="inline">Исполнитель отключён на сервере. Расписание можно сохранить для следующего включения.</Notice>}
    <p className="text-sm text-text-secondary">Общее расписание всех автоматических подключений Forge. Одновременно выполняется синхронизаций: {baseline.concurrency}.</p>
    <form aria-label="Расписание синхронизации" onSubmit={(event) => void save(event)} className="space-y-3">
      <Fieldset disabled={pending || uncertain}>
        <FormField id="sync-interval-seconds" label="Период проверки, секунд" hint="3600 секунд — один час. Допустимо от 1 до 86400 секунд.">
          {(attrs) => <Input {...attrs} type="number" min={1} max={86400} step={1} required
            value={seconds} onChange={(event) => setSeconds(event.target.value)} />}
        </FormField>
        <Checkbox label="Запускать также после изменений в Forge" checked={onChange} onCheckedChange={(value) => setOnChange(value === true)} />
      </Fieldset>
      <p className="text-xs text-text-secondary">При выключенном запуске после изменений действует только расписание. Ручной запуск остаётся доступен; начатые и уже поставленные в очередь запуски продолжаются.</p>
      {error && <Notice role="alert" tone="warning">{error}</Notice>}
      {message && <Notice role="status" tone="success">{message}</Notice>}
      <ActionGroup align="start">
        <Button type="submit" disabled={pending || (!uncertain && (!dirty || !valid))}>{uncertain && !pending ? "Проверить сохранение" : "Сохранить расписание"}</Button>
        <Button type="button" variant="outline" disabled={pending} onClick={() => void refresh()}>{dirty || uncertain ? "Отменить правки и обновить" : "Обновить расписание"}</Button>
      </ActionGroup>
    </form>
  </>;
}
