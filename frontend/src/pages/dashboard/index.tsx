import {
  Metric,
  Card,
  CardContent,
  Notice,
  SectionHeader,
  LoadingState,
  PageHeader,
  NamespaceLink as Link,
  ErrorState,
} from "@pdlc/ui/ui";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/api/client";
import type { components } from "@/api/schema";
import { ExecutionList } from "@/widgets/execution-list";

import type { Runner } from "@/api/types";
export function DashboardPage() {
  const runners = useQuery({
    queryKey: ["runners"],
    queryFn: ({ signal }) => api<Runner[]>("/runners", { signal }),
  });
  const summary = useQuery({
    queryKey: ["workspace-summary", "all"],
    queryFn: ({ signal }) =>
      api<components["schemas"]["WorkspaceSummary"]>("/workspace-summary", {
        signal,
      }),
  });
  return (
    <div className="space-y-5">
      <PageHeader
        title="CI/CD"
        context={
          <>
            <p>Показатели относятся к подключённым проектам.</p>
            <p className="flex flex-wrap gap-3">
              <Link to="/repositories" className="text-accent underline">
                Все репозитории
              </Link>
              <Link to="/delivery-configs" className="text-accent underline">
                Все CI-конфигурации
              </Link>
            </p>
          </>
        }
      />
      {summary.isPending ? (
        <LoadingState message="Загружаем сводку…" />
      ) : summary.isError ? (
        <ErrorState message="Сводка недоступна." />
      ) : (
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {[
            ["Проекты", summary.data.projects],
            ["Репозитории", summary.data.repositories],
            ["CI-конфигурации", summary.data.configurations],
            ["В очереди", summary.data.queued],
            ["Выполняются", summary.data.running],
            ["С ошибкой", summary.data.failed],
          ].map(([label, count]) => (
            <Metric key={label} label={label} value={count} />
          ))}
        </div>
      )}
      <Card asChild>
        <section>
          <CardContent>
            <Link to="/runners" className="font-semibold text-accent">
              Исполнители CI
            </Link>
            {runners.isError ? (
              <Notice tone="danger" appearance="inline">
                Состояние исполнителей недоступно.
              </Notice>
            ) : runners.isPending ? (
              <LoadingState message="Загружаем исполнителей…" />
            ) : (
              <p className="text-sm text-text-muted">
                Онлайн:{" "}
                {
                  runners.data.filter((runner) => runner.status === "online")
                    .length
                }{" "}
                / {runners.data.length}
              </p>
            )}
          </CardContent>
        </section>
      </Card>
      <SectionHeader title={<>Пайплайны проектов</>} />
      <ExecutionList />
    </div>
  );
}
