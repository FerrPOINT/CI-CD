import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ActionGroup, Badge, Button, FormField, Input, Notice, Prose,
  ResourceList, ResourceListDescription, ResourceListRow, SectionHeader,
  Select, Textarea, TextLink,
} from "@pdlc/ui/ui";
import { withNamespaceLocation } from "@pdlc/ui/lib";
import { usePullReviewContext } from "@/api/hooks";
import { api, ApiError, downloadPullPatch, saveDownloadedArtifact } from "@/api/client";
import type { Status } from "@/api/types";

type Anchor = { path: string; side: "old" | "new"; line: number };
type CommentInput = {
  operation_id: string; base_sha: string; head_sha: string;
  body: string; severity: string; anchor: Anchor | null;
};
type Comment = CommentInput & { id: string; author: string | null; created_at: string };

export function PullComments({ repositoryId, number, repo, namespace }: {
  repositoryId: string; number: number; repo: string;
  namespace?: { registry_instance_id: string; namespace_id: string } | null;
}) {
  const { t } = useTranslation();
  const root = `/catalog/repositories/${repositoryId}/pulls/${number}`;
  const cache = useQueryClient();
  const queryKey = ["pull-comments", repositoryId, number];
  const context = usePullReviewContext(repositoryId, number);
  const comments = useQuery({ queryKey, queryFn: async ({ signal }) => {
    const all: Comment[] = [];
    for (let offset = 0; ; offset += 100) {
      const page = await api<Comment[]>(`${root}/comments?limit=100&offset=${offset}`, { signal });
      all.push(...page);
      if (page.length < 100) return all;
    }
  }});
  const headSha = context.data?.head_sha;
  const checks = useQuery({
    queryKey: ["pull-review-checks", repositoryId, number, headSha], enabled: Boolean(headSha),
    queryFn: async ({ signal }) => {
      const all: { pipeline_id: string; commit_sha: string; status: Status }[] = [];
      for (let offset = 0; ; offset += 100) {
        const page = await api<typeof all>(`${root}/checks?head_sha=${headSha}&limit=100&offset=${offset}`, { signal });
        all.push(...page);
        if (page.length < 100) return all;
      }
    },
  });
  const [downloading, setDownloading] = useState(false);
  const [downloadError, setDownloadError] = useState<string>();
  async function downloadPatch() {
    if (!context.data || downloading) return;
    setDownloading(true); setDownloadError(undefined);
    try {
      const patch = await downloadPullPatch(repositoryId, number, context.data.base_sha, context.data.head_sha);
      saveDownloadedArtifact(patch);
    } catch (failure) {
      setDownloadError(failure instanceof Error ? failure.message : "Не удалось скачать diff");
    } finally { setDownloading(false); }
  }
  const [body, setBody] = useState("");
  const [severity, setSeverity] = useState("note");
  const [path, setPath] = useState("");
  const [side, setSide] = useState("new");
  const [line, setLine] = useState("");
  const [pending, setPending] = useState<CommentInput | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!context.data || saving) return;
    const input = pending ?? {
      operation_id: crypto.randomUUID(), base_sha: context.data.base_sha,
      head_sha: context.data.head_sha, body, severity,
      anchor: path ? { path, side: side as Anchor["side"], line: Number(line) } : null,
    };
    setPending(input); setSaving(true); setError(undefined);
    try {
      const observed = pending ? await api<Comment[]>(`${root}/comments?operation_id=${pending.operation_id}`) : [];
      if (observed.length === 0) {
        await api<Comment>(`${root}/comments`, { method: "POST", body: JSON.stringify(input) });
      }
      setPending(null); setBody(""); setPath(""); setLine("");
      await cache.invalidateQueries({ queryKey });
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "Не удалось сохранить замечание");
      if (failure instanceof ApiError && failure.kind === "api" && failure.status >= 400 && failure.status < 500) {
        setPending(null);
        await context.refetch();
      }
    } finally { setSaving(false); }
  }

  return <section className="space-y-4" aria-label="Замечания к PR">
    <SectionHeader title="Замечания к PR" />
    {context.data && <ResourceListDescription technical>
      <TextLink href={withNamespaceLocation(`/repositories/${encodeURIComponent(repo)}/compare?from=${context.data.base_sha}&to=${context.data.head_sha}`, namespace ?? null)}>
        Открыть diff проверяемой версии
      </TextLink> · {context.data.head_sha}
    </ResourceListDescription>}
    <ActionGroup align="start"><Button variant="outline" disabled={!context.data || downloading} onClick={() => void downloadPatch()}>
      {downloading ? "Скачиваем…" : "Скачать полный diff"}
    </Button></ActionGroup>
    {downloadError && <Notice tone="danger" appearance="inline" role="alert">{downloadError}</Notice>}
    <SectionHeader title="Готовые проверки этой версии" />
    {checks.isError && <Notice tone="danger" appearance="inline">Не удалось прочитать проверки для этого SHA.</Notice>}
    {checks.isSuccess && checks.data.length === 0 && <ResourceListDescription>Для этой версии готовых проверок нет.</ResourceListDescription>}
    <ResourceList>{checks.data?.map((check) => <ResourceListRow key={check.pipeline_id}>
      <TextLink href={withNamespaceLocation(`/pipelines/${check.pipeline_id}`, namespace ?? null)}>Проверка {check.pipeline_id.slice(0, 8)}</TextLink>
      <Badge>{t(`pipelines.${check.status}`)}</Badge>
    </ResourceListRow>)}</ResourceList>
    <SectionHeader title="Сохранённые замечания" />
    {comments.isError && <Notice tone="danger" appearance="inline" role="alert">Не удалось прочитать замечания.</Notice>}
    <ResourceList>
      {comments.data?.map((comment) => {
        const outdated = context.data && (comment.base_sha !== context.data.base_sha || comment.head_sha !== context.data.head_sha);
        return <ResourceListRow key={comment.id}>
          <div className="min-w-0 space-y-2" id={`comment-${comment.id}`}>
            <p>{comment.author ?? "Участник"} · {new Date(comment.created_at).toLocaleString("ru-RU")} · <Badge>{comment.severity === "note" ? "Примечание" : comment.severity}</Badge>
              {outdated && <Badge>Предыдущая версия</Badge>}</p>
            {comment.anchor && <TextLink href={withNamespaceLocation(`/repositories/${encodeURIComponent(repo)}/compare?from=${comment.base_sha}&to=${comment.head_sha}`, namespace ?? null)}>
              {comment.anchor.path}:{comment.anchor.line} · {comment.anchor.side === "old" ? "до изменения" : "после изменения"}
            </TextLink>}
            <Prose><p className="whitespace-pre-wrap">{comment.body}</p></Prose>
            <ResourceListDescription technical>Commit: {comment.head_sha}</ResourceListDescription>
          </div>
        </ResourceListRow>;
      })}
    </ResourceList>
    {context.isError && <Notice tone="danger" appearance="inline">Версия PR недоступна. Сохранённые замечания остаются в истории.</Notice>}
    <form onSubmit={(event) => void submit(event)} className="space-y-3" aria-label="Новое замечание">
      <FormField id={`comment-body-${repositoryId}`} label="Замечание" required>
        {(attrs) => <Textarea {...attrs} value={body} disabled={Boolean(pending)} onChange={(event) => setBody(event.target.value)} />}
      </FormField>
      <div className="grid gap-3 sm:grid-cols-2">
        <FormField id={`comment-severity-${repositoryId}`} label="Важность">
          {(attrs) => <Select {...attrs} value={severity} disabled={Boolean(pending)} onValueChange={setSeverity}
            options={[{ value: "note", label: "Примечание" }, ...["P0", "P1", "P2", "P3"].map((value) => ({ value, label: value }))]} />}
        </FormField>
        <FormField id={`comment-path-${repositoryId}`} label="Файл" hint="Оставьте пустым для общего замечания">
          {(attrs) => <Input {...attrs} value={path} disabled={Boolean(pending)} onChange={(event) => setPath(event.target.value)} />}
        </FormField>
        {path && <>
          <FormField id={`comment-side-${repositoryId}`} label="Сторона diff">
            {(attrs) => <Select {...attrs} value={side} disabled={Boolean(pending)} onValueChange={setSide}
              options={[{ value: "old", label: "До изменения" }, { value: "new", label: "После изменения" }]} />}
          </FormField>
          <FormField id={`comment-line-${repositoryId}`} label="Строка" required>
            {(attrs) => <Input {...attrs} type="number" min={1} value={line} disabled={Boolean(pending)} onChange={(event) => setLine(event.target.value)} />}
          </FormField>
        </>}
      </div>
      {error && <Notice role="alert" tone="danger" appearance="inline">{error}</Notice>}
      {pending && !saving && <Notice appearance="inline">Результат записи пока неизвестен. Повтор сохранит тот же ключ и текст.</Notice>}
      <ActionGroup align="start"><Button type="submit" disabled={saving || !context.data || !body.trim()}>
        {saving ? "Сохраняем…" : pending ? "Повторить сохранение" : "Добавить замечание"}
      </Button></ActionGroup>
    </form>
  </section>;
}
