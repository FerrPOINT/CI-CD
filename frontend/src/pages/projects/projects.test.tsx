import { act, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProjectsPage } from "./index";

const mocks = vi.hoisted(() => ({
  useProjects: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  remove: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { name?: string }) =>
      options?.name ? `${key} ${options.name}` : key,
  }),
}));
vi.mock("@/api/hooks", () => ({
  useProjects: mocks.useProjects,
  useCreateProject: () => ({ mutate: mocks.create, isPending: false }),
  useUpdateProject: () => ({ mutate: mocks.update, isPending: false }),
  useDeleteProject: () => ({ mutate: mocks.remove, isPending: false }),
}));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

function project(index: number) {
  return {
    id: `project-${index}`,
    name: `Project ${String(index).padStart(2, "0")}`,
    repository_url: `https://example.test/project-${index}.git`,
    default_branch: "main",
    created_at: "2026-09-19T00:00:00Z",
  };
}

function setup(count: number) {
  mocks.useProjects.mockReturnValue({
    data: Array.from({ length: count }, (_, index) => project(index + 1)),
    isLoading: false,
    error: null,
  });
  render(
    <MemoryRouter>
      <ProjectsPage />
    </MemoryRouter>,
  );
}

afterEach(() => vi.clearAllMocks());

describe("ProjectsPage", () => {
  it("keeps pipelines direct and exposes secondary destinations in an accessible menu", () => {
    setup(2);

    expect(
      screen.getByRole("link", { name: "projects.openPipelines Project 01" }),
    ).toHaveAttribute("href", "/projects/project-1/pipelines");
    expect(
      screen.queryByRole("link", { name: "secrets.title" }),
    ).not.toBeInTheDocument();
    fireEvent.keyDown(
      screen.getByRole("button", { name: "projects.actionsFor Project 01" }),
      { key: "Enter" },
    );
    expect(
      screen.getByRole("menuitem", { name: "projects.repositories" }),
    ).toHaveAttribute("href", "/repositories?project=Project%2001");
    expect(
      screen.getByRole("menuitem", { name: "secrets.title" }),
    ).toHaveAttribute("href", "/projects/project-1/secrets");
    expect(
      screen.getByRole("menuitem", { name: "environments.title" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("menuitem", { name: "schedules.title" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("menuitem", { name: "webhooks.title" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("menuitem", { name: "reports.title" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "projects.edit" }));
    expect(
      screen.getByRole("form", { name: "projects.editProject Project 01" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("projects.repositoryUrl")).toHaveValue(
      "https://example.test/project-1.git",
    );
  });

  it("searches and paginates a growing project catalog", () => {
    setup(25);

    expect(
      screen.getAllByRole("link", { name: /projects.openPipelines/ }),
    ).toHaveLength(12);
    fireEvent.click(screen.getByRole("button", { name: "projects.next" }));
    fireEvent.click(screen.getByRole("button", { name: "projects.next" }));
    expect(
      screen.getAllByRole("link", { name: /projects.openPipelines/ }),
    ).toHaveLength(1);
    expect(screen.getByText("3 / 3")).toBeInTheDocument();
    fireEvent.change(
      screen.getByRole("searchbox", { name: "projects.search" }),
      { target: { value: "project-1.git" } },
    );
    expect(
      screen.getAllByRole("link", { name: /projects.openPipelines/ }),
    ).toHaveLength(1);
    expect(
      screen.queryByRole("navigation", { name: "projects.pages" }),
    ).not.toBeInTheDocument();
  });

  it("opens the create form on demand and closes it after success", () => {
    setup(0);

    expect(
      screen.queryByRole("form", { name: "projects.create" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "projects.create" }));
    const form = screen.getByRole("form", { name: "projects.create" });
    fireEvent.change(screen.getByLabelText("projects.name"), {
      target: { value: "New project" },
    });
    fireEvent.change(screen.getByLabelText("projects.repositoryUrl"), {
      target: { value: "https://example.test/new.git" },
    });
    fireEvent.submit(form);
    expect(mocks.create).toHaveBeenCalledWith(
      {
        name: "New project",
        repository_url: "https://example.test/new.git",
        default_branch: "main",
      },
      { onSuccess: expect.any(Function), onError: expect.any(Function) },
    );
    act(() => mocks.create.mock.calls[0][1].onSuccess());
    expect(
      screen.queryByRole("form", { name: "projects.create" }),
    ).not.toBeInTheDocument();
  });

  it("keeps delete confirmation open after an API error and closes after success", () => {
    setup(1);

    fireEvent.keyDown(
      screen.getByRole("button", { name: "projects.actionsFor Project 01" }),
      {
        key: "Enter",
      },
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "common.delete" }));
    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    expect(mocks.remove).toHaveBeenCalledWith("project-1", {
      onSuccess: expect.any(Function),
      onError: expect.any(Function),
    });
    act(() => mocks.remove.mock.calls[0][1].onError(new Error("Unavailable")));
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();

    act(() => mocks.remove.mock.calls[0][1].onSuccess());
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });
});
