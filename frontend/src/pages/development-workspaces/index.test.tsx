import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { ApiError } from "@/api/client";
import {
  developmentWorkspaces as service,
  type DevelopmentWorkspace,
  type WorkspaceDetail,
  type WorkspaceOperation,
} from "@/api/development-workspaces";
import { DevelopmentWorkspacesPage } from "./index";

const context = vi.hoisted(() => ({
  ref: {
    registry_instance_id: "11111111-1111-4111-8111-111111111111",
    namespace_id: "22222222-2222-4222-8222-222222222222",
  },
}));
vi.mock("@/widgets/namespace-context", () => ({
  useNamespaceRef: () => context,
}));
vi.mock("@pdlc/ui/ui", async () => ({
  ...(await vi.importActual<typeof import("@pdlc/ui/ui")>("@pdlc/ui/ui")),
  usePlatformServices: () => ({
    services: [
      {
        key: "task-tracker",
        ui_url: "https://platform.example.invalid/tracker",
      },
    ],
  }),
}));
const configuration = "33333333-3333-4333-8333-333333333333";
const repository = "44444444-4444-4444-8444-444444444444";
const id = "55555555-5555-4555-8555-555555555555";
const actor = "66666666-6666-4666-8666-666666666666";
const now = "2026-10-31T12:00:00Z";
const workspace: DevelopmentWorkspace = {
  id,
  project_id: configuration,
  repository_id: repository,
  environment_id: "77777777-7777-4777-8777-777777777777",
  environment_hash: "a".repeat(64),
  tracker_instance_id: context.ref.registry_instance_id,
  tracker_project_id: context.ref.namespace_id,
  name: "R1 isolated workspace",
  profile_id: id,
  target_id: id,
  catalog_target_id: id,
  profile_hash: "b".repeat(64),
  catalog_hash: "c".repeat(64),
  public_origin: "https://workspace.example.invalid",
  profile: { name: "r1", checks: [{ key: "backend-unit", executor: "forge" }] },
  sources: [{ key: "r1", repository_id: repository, commit: "d".repeat(40) }],
  local_preparation: {},
  preparation: "prepared",
  runtime: "stopped",
  desired_runtime: "stopped",
  observation: {},
  observed_at: now,
  version: 7,
  created_by: actor,
  owner_name: "workspace-developer",
  created_at: now,
};
const operation: WorkspaceOperation = {
  id: "88888888-8888-4888-8888-888888888888",
  workspace_id: id,
  actor_id: actor,
  request: { action: "start" },
  expected_version: 7,
  state: "unknown",
  created_at: now,
};
function detail(overrides: Partial<WorkspaceDetail> = {}): WorkspaceDetail {
  return {
    workspace,
    actor_id: actor,
    session: null,
    latest_operation: null,
    actions: [
      "observe",
      "start",
      "stop",
      "begin_session",
      "prepare",
      "check",
      "build",
      "reconcile",
    ].map((action) => ({ action, allowed: true, reason: null })),
    ...overrides,
  };
}
const config = {
  id: configuration,
  name: "Development",
  repository_id: repository,
  repository_name: "r1/r1",
  repository_url: "https://forge.example.invalid/git/r1/r1.git",
  default_branch: "main",
  created_at: now,
};
beforeEach(() => {
  vi.spyOn(service, "configurations").mockResolvedValue({
    items: [config],
    total: 1,
  });
  vi.spyOn(service, "configuration").mockResolvedValue(config);
  vi.spyOn(service, "repository").mockResolvedValue({
    id: repository,
    public_name: "r1/r1",
    slug: "r1",
    storage_name: "repository-storage",
    kind: "local",
    namespace: context.ref,
    visibility: "private",
    ready: true,
    provider_refs: {},
  });
  vi.spyOn(service, "targets").mockResolvedValue({
    items: [],
    can_register: false,
    denied_reason: "Требуются права сопровождающего проекта",
  });
  vi.spyOn(service, "list").mockResolvedValue({ items: [workspace], total: 1 });
  vi.spyOn(service, "detail").mockResolvedValue(detail());
  vi.spyOn(service, "operations").mockResolvedValue([]);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
function mount() {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={cache}>
      <MemoryRouter
        initialEntries={[
          `/?workspace_configuration=${configuration}&development_workspace=${id}`,
        ]}
      >
        <DevelopmentWorkspacesPage />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

it("keeps runtime, source readiness and current session separate", async () => {
  mount();
  expect(await screen.findByRole("button", { name: "Включить" })).toBeEnabled();
  expect(screen.getByText("Подготовлено")).toBeVisible();
  expect(screen.getByText("Выключено")).toBeVisible();
  expect(screen.getByText("Свободно")).toBeVisible();
  expect(
    screen.getByText("Владелец окружения: workspace-developer"),
  ).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Открыть приложение" }),
  ).toHaveAttribute("href", workspace.public_origin);
});

it("shows the named session owner and opens its real Tracker task", async () => {
  vi.mocked(service.detail).mockResolvedValue(
    detail({
      session: {
        id,
        workspace_id: id,
        actor_id: actor,
        actor_name: "session-developer",
        owner_subject: actor,
        task_id: repository,
        task_ref: "DEV-7",
        sources: workspace.sources,
        started_at: now,
      },
    }),
  );
  mount();
  const task = await screen.findByRole("link", { name: "DEV-7" });
  expect(task).toHaveAttribute(
    "href",
    "https://platform.example.invalid/tracker/issues/DEV-7",
  );
  expect(screen.getByText(/Владелец: session-developer/)).toBeVisible();
});

it("retains the exact UUID, version and payload after a lost reply", async () => {
  const submit = vi
    .spyOn(service, "submit")
    .mockRejectedValueOnce(
      new ApiError({ kind: "network", message: "reply lost" }),
    )
    .mockResolvedValueOnce({ ...operation, state: "queued" });
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "Включить" }));
  await screen.findByRole("button", { name: "Повторить исходный запрос" });
  expect(screen.getByRole("button", { name: "Выключить" })).toBeDisabled();
  fireEvent.click(
    screen.getByRole("button", { name: "Повторить исходный запрос" }),
  );
  await waitFor(() => expect(submit).toHaveBeenCalledTimes(2));
  expect(submit.mock.calls[1]).toEqual(submit.mock.calls[0]);
  expect(submit.mock.calls[0][1]).toEqual({
    operation_id: expect.stringMatching(/^[a-f0-9-]{36}$/),
    expected_version: 7,
    command: { action: "start" },
  });
});

it("reads the original operation without submitting a replacement command", async () => {
  const submit = vi
    .spyOn(service, "submit")
    .mockRejectedValue(
      new ApiError({ kind: "network", message: "reply lost" }),
    );
  const read = vi.spyOn(service, "operation").mockResolvedValue(operation);
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "Включить" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Прочитать исходную операцию" }),
  );
  await waitFor(() =>
    expect(read).toHaveBeenCalledWith(id, submit.mock.calls[0][1].operation_id),
  );
  expect(submit).toHaveBeenCalledTimes(1);
});

it("shows the server denial and never submits a disabled stop", async () => {
  vi.mocked(service.detail).mockResolvedValue(
    detail({
      actions: [
        {
          action: "stop",
          allowed: false,
          reason: "Сначала закончите рабочую сессию",
        },
      ],
    }),
  );
  const submit = vi.spyOn(service, "submit");
  mount();
  expect(
    await screen.findByRole("button", { name: "Выключить" }),
  ).toBeDisabled();
  expect(screen.getByText("Сначала закончите рабочую сессию")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Выключить" }));
  expect(submit).not.toHaveBeenCalled();
});

it("restores an unknown original operation explicitly after refresh", async () => {
  vi.mocked(service.detail).mockResolvedValue(
    detail({
      latest_operation: operation,
      actions: [
        {
          action: "start",
          allowed: false,
          reason: "Сначала восстановите текущую операцию",
        },
        { action: "reconcile", allowed: true, reason: null },
      ],
    }),
  );
  const reconcile = vi
    .spyOn(service, "reconcile")
    .mockResolvedValue({ ...operation, state: "queued" });
  mount();
  expect(
    await screen.findByRole("button", { name: "Включить" }),
  ).toBeDisabled();
  fireEvent.click(
    screen.getByRole("button", { name: "Восстановить исходную операцию" }),
  );
  await waitFor(() => expect(reconcile).toHaveBeenCalledWith(id, operation.id));
});

it("does not load workspaces selected from another project namespace", async () => {
  vi.mocked(service.repository).mockResolvedValue({
    id: repository,
    public_name: "other/product",
    slug: "other",
    kind: "local",
    namespace: {
      ...context.ref,
      namespace_id: "99999999-9999-4999-8999-999999999999",
    },
    visibility: "private",
    ready: true,
    provider_refs: {},
  });
  mount();
  await screen.findByText(
    "Выбранная конфигурация принадлежит другому проекту. Выберите конфигурацию текущего проекта.",
  );
  expect(service.list).not.toHaveBeenCalled();
  expect(service.detail).not.toHaveBeenCalled();
});

it("distinguishes an unavailable card from an unknown runtime", async () => {
  vi.mocked(service.detail).mockRejectedValue(
    new ApiError({ kind: "network", message: "offline" }),
  );
  mount();
  await screen.findByText(
    "Сервер недоступен. Результат отправленной операции нужно прочитать по исходному ключу.",
  );
  expect(
    screen.queryByRole("button", { name: "Включить" }),
  ).not.toBeInTheDocument();
});
