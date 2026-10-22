import { applicationAccessFixture } from "@/test/application-access";
const accessActor = vi.hoisted(() => ({
  role: "user" as "user" | "admin",
  applications: ["ci-cd"],
}));
vi.mock("@pdlc/ui/auth", async (original) => ({
  ...(await original<object>()),
  useApplicationAccess: () => applicationAccessFixture(accessActor),
}));
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { PlatformServicesProvider } from "@pdlc/ui/lib";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ProjectsPage } from "./index";
import { api } from "@/api/client";
vi.mock("@/api/client", () => ({ api: vi.fn() }));
const registry = "11111111-1111-4111-8111-111111111111";
const namespaces = [
  "22222222-2222-4222-8222-222222222222",
  "33333333-3333-4333-8333-333333333333",
];
function project(index: number) {
  return {
    registry_instance_id: registry,
    namespace_id: namespaces[index],
    tracker_instance_id: registry,
    tracker_project_id: namespaces[index],
    name: "Same name",
    project_key: index ? "TWO" : "ONE",
    state: "active",
    stale: false,
    group_id: index ? null : registry,
    group_slug: "git-group",
    repositories: 3,
    latest_status: null,
  };
}
function mount() {
  vi.stubGlobal(
    "fetch",
    vi.fn(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            services: [
              {
                key: "admin-panel",
                label: "Own Admin",
                url: "http://localhost:8771",
                ui_url: "http://localhost:8772",
                health: "unknown",
              },
            ],
          }),
        ),
      ),
    ),
  );
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <PlatformServicesProvider catalogUrl="https://admin.own.test/api/v1/runtime/services">
        <MemoryRouter initialEntries={["/projects?project_scope=all"]}>
          <ProjectsPage />
        </MemoryRouter>
      </PlatformServicesProvider>
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  accessActor.role = "user";
});
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
  vi.unstubAllGlobals();
});
describe("shared workspace catalog", () => {
  it("uses Tracker keys and stable Namespace routes for equally named projects", async () => {
    accessActor.role = "admin";
    vi.mocked(api).mockResolvedValue({
      items: [project(0), project(1)],
      total: 2,
    });
    mount();
    await screen.findByText("Git не подключён");
    const links = screen
      .getAllByRole("link")
      .filter((link) => link.textContent?.includes("Same name"));
    expect(links).toHaveLength(2);
    links.forEach((link, index) => {
      expect(link.getAttribute("href")).toContain(
        `/workspaces/${registry}/${namespaces[index]}`,
      );
      expect(
        link.querySelector(`[data-project-avatar="${index ? "TWO" : "ONE"}"]`),
      ).not.toBeNull();
    });
    expect(
      screen.queryByRole("button", { name: /Удалить|Создать/ }),
    ).toBeNull();
    expect(
      await screen.findByRole("link", { name: "Создать проект" }),
    ).toHaveAttribute(
      "href",
      "http://localhost:8772/namespaces?project_scope=all",
    );
  });
  it("searches and paginates on the server beyond the first page", async () => {
    vi.mocked(api).mockImplementation(async (path) => ({
      items: [project(path.includes("offset=50") ? 1 : 0)],
      total: 51,
    }));
    mount();
    await screen.findByText("Репозитории: 3 · Git-группа git-group");
    fireEvent.click(screen.getByRole("button", { name: "Далее" }));
    await screen.findByText("Git не подключён");
    expect(
      vi.mocked(api).mock.calls.some(([path]) => path.includes("offset=50")),
    ).toBe(true);
    fireEvent.change(screen.getByRole("textbox", { name: "Поиск проектов" }), {
      target: { value: "ONE" },
    });
    await waitFor(() =>
      expect(
        vi
          .mocked(api)
          .mock.calls.some(([path]) => path.includes("offset=0&search=ONE")),
      ).toBe(true),
    );
  });
  it("distinguishes an unavailable source from a successful empty catalog", async () => {
    vi.mocked(api).mockRejectedValue(new Error("Tracker unavailable"));
    mount();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Каталог проектов недоступен",
    );
    expect(screen.queryByText(/Подключённых проектов пока нет/)).toBeNull();
  });

  it("keeps shared work visible without offering structure creation to a current user", async () => {
    vi.mocked(api).mockResolvedValue({
      items: [project(0), project(1)],
      total: 2,
    });
    mount();
    await screen.findByText("Git не подключён");
    expect(screen.getAllByText("Same name")).toHaveLength(2);
    expect(
      screen.queryByRole("link", { name: "Создать проект" }),
    ).not.toBeInTheDocument();
  });
});
