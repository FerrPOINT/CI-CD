import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router";
import {
  BarChart3,
  ChevronRight,
  Clock,
  FolderGit2,
  GitFork,
  Globe,
  KeyRound,
  MoreHorizontal,
  Pencil,
  Plus,
  Search,
  Trash2,
  Webhook,
} from "lucide-react";
import { toast } from "sonner";
import {
  useCreateProject,
  useDeleteProject,
  useProjects,
  useUpdateProject,
} from "@/api/hooks";
import type { Project } from "@/api/types";
import {
  Button,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Input,
  Label,
} from "@sdlc/ui/ui";
import { ConfirmDialog } from "@/shared/ui/confirm-dialog";
import { QueryState } from "@/shared/ui/query-state";

const pageSize = 12;
const emptyForm = { name: "", repository_url: "", default_branch: "main" };

export function ProjectsPage() {
  const { t } = useTranslation();
  const {
    data: projectsData,
    isLoading,
    error: listError,
    refetch,
  } = useProjects();
  const projects = projectsData ?? [];
  const hasProjectData = projectsData !== undefined;
  const createProject = useCreateProject();
  const updateProject = useUpdateProject();
  const deleteProject = useDeleteProject();
  const [showForm, setShowForm] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<Project | null>(null);
  const [form, setForm] = useState(emptyForm);
  const [editing, setEditing] = useState<Project | null>(null);
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(1);

  const normalizedSearch = search.trim().toLocaleLowerCase();
  const filteredProjects = projects
    .filter((project) =>
      `${project.name} ${project.repository_url}`
        .toLocaleLowerCase()
        .includes(normalizedSearch),
    )
    .sort((a, b) => a.name.localeCompare(b.name));
  const totalPages = Math.max(1, Math.ceil(filteredProjects.length / pageSize));
  const currentPage = Math.min(page, totalPages);
  const visibleProjects = filteredProjects.slice(
    (currentPage - 1) * pageSize,
    currentPage * pageSize,
  );

  function handleCreate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    createProject.mutate(form, {
      onSuccess: () => {
        setShowForm(false);
        setForm(emptyForm);
        toast.success(t("projects.created"));
      },
      onError: (error) => toast.error(error.message),
    });
  }

  function handleUpdate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!editing) return;
    updateProject.mutate(
      {
        id: editing.id,
        name: editing.name,
        repository_url: editing.repository_url,
        default_branch: editing.default_branch,
      },
      {
        onSuccess: () => {
          setEditing(null);
          toast.success(t("projects.updated"));
        },
        onError: (error) => toast.error(error.message),
      },
    );
  }

  return (
    <div className="space-y-5">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-2xl font-bold">{t("projects.title")}</h1>
        <Button
          size="sm"
          className="min-h-10 sm:min-h-10"
          aria-expanded={showForm}
          disabled={createProject.isPending}
          onClick={() => {
            setEditing(null);
            setShowForm((value) => !value);
          }}
        >
          <Plus className="h-4 w-4" aria-hidden />
          {t("projects.create")}
        </Button>
      </header>

      {showForm && (
        <form
          onSubmit={handleCreate}
          aria-label={t("projects.create")}
          className="grid gap-3 border-y border-border py-4 sm:grid-cols-2 lg:grid-cols-3"
        >
          <div className="space-y-1.5">
            <Label htmlFor="project-name">{t("projects.name")}</Label>
            <Input
              id="project-name"
              className="min-h-10"
              required
              value={form.name}
              onChange={(event) =>
                setForm({ ...form, name: event.target.value })
              }
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="project-repo">{t("projects.repositoryUrl")}</Label>
            <Input
              id="project-repo"
              className="min-h-10"
              required
              placeholder="git@github.com:org/repo.git"
              value={form.repository_url}
              onChange={(event) =>
                setForm({ ...form, repository_url: event.target.value })
              }
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="project-branch">
              {t("projects.defaultBranch")}
            </Label>
            <Input
              id="project-branch"
              className="min-h-10"
              required
              value={form.default_branch}
              onChange={(event) =>
                setForm({ ...form, default_branch: event.target.value })
              }
            />
          </div>
          <div className="flex flex-wrap justify-end gap-2 sm:col-span-2 lg:col-span-3">
            <Button
              type="button"
              variant="outline"
              className="min-h-10 sm:min-h-10"
              disabled={createProject.isPending}
              onClick={() => setShowForm(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="submit"
              className="min-h-10 sm:min-h-10"
              disabled={createProject.isPending}
            >
              {t("projects.create")}
            </Button>
          </div>
        </form>
      )}

      {listError && hasProjectData && (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-3 border-l-2 border-danger py-2 pl-3 text-sm text-text-secondary"
        >
          <span className="min-w-0 flex-1">
            {t("projects.refreshError")}
          </span>
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="min-h-10 sm:min-h-10"
            onClick={() => {
              void refetch();
            }}
          >
            {t("common.retry")}
          </Button>
        </div>
      )}

      <QueryState
        data={projectsData}
        isLoading={isLoading}
        error={hasProjectData ? null : listError}
        errorMessage={t("projects.loadError")}
        onRetry={() => {
          void refetch();
        }}
        isEmpty={(list) => list.length === 0}
        empty={{ title: t("projects.empty") }}
      >
        {() => (
          <section aria-label={t("projects.title")} className="space-y-3">
            <div className="relative">
              <Search
                className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted"
                aria-hidden
              />
              <Input
                type="search"
                aria-label={t("projects.search")}
                placeholder={t("projects.search")}
                className="min-h-10 pl-9"
                value={search}
                onChange={(event) => {
                  setSearch(event.target.value);
                  setPage(1);
                }}
              />
            </div>
            <p className="text-xs text-text-muted">
              {t("projects.shown", {
                count: visibleProjects.length,
                total: filteredProjects.length,
              })}
            </p>
            {filteredProjects.length === 0 ? (
              <p
                role="status"
                className="border-y border-border py-6 text-sm text-text-muted"
              >
                {t("projects.noMatches")}
              </p>
            ) : (
              <ul className="divide-y divide-border border-y border-border">
                {visibleProjects.map((project) => (
                  <li key={project.id}>
                    <div className="flex min-w-0 items-center gap-2">
                      <Link
                        to={`/projects/${project.id}/pipelines`}
                        aria-label={t("projects.openPipelines", {
                          name: project.name,
                        })}
                        className="flex min-h-16 min-w-0 flex-1 items-center gap-3 py-2 hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent"
                      >
                        <FolderGit2
                          className="h-4 w-4 shrink-0 text-accent"
                          aria-hidden
                        />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-sm font-medium">
                            {project.name}
                          </span>
                          <span
                            className="block truncate text-xs text-text-muted"
                            title={project.repository_url}
                          >
                            {project.repository_url}
                          </span>
                          <span className="block truncate text-xs text-text-secondary">
                            {project.default_branch}
                          </span>
                        </span>
                        <ChevronRight
                          className="h-4 w-4 shrink-0 text-text-muted"
                          aria-hidden
                        />
                      </Link>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <button
                            type="button"
                            aria-label={t("projects.actionsFor", {
                              name: project.name,
                            })}
                            className="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-md text-text-secondary hover:bg-surface-raised hover:text-text-primary focus-visible:outline-2 focus-visible:outline-accent"
                          >
                            <MoreHorizontal className="h-5 w-5" aria-hidden />
                          </button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-52">
                          <DropdownMenuItem
                            asChild
                            className="min-h-10 sm:min-h-10"
                          >
                            <Link
                              to={`/repositories?project=${encodeURIComponent(project.name)}`}
                            >
                              <GitFork className="mr-2 h-4 w-4" aria-hidden />
                              {t("projects.repositories")}
                            </Link>
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            asChild
                            className="min-h-10 sm:min-h-10"
                          >
                            <Link to={`/projects/${project.id}/secrets`}>
                              <KeyRound className="mr-2 h-4 w-4" aria-hidden />
                              {t("secrets.title")}
                            </Link>
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            asChild
                            className="min-h-10 sm:min-h-10"
                          >
                            <Link to={`/projects/${project.id}/environments`}>
                              <Globe className="mr-2 h-4 w-4" aria-hidden />
                              {t("environments.title")}
                            </Link>
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            asChild
                            className="min-h-10 sm:min-h-10"
                          >
                            <Link to={`/projects/${project.id}/schedules`}>
                              <Clock className="mr-2 h-4 w-4" aria-hidden />
                              {t("schedules.title")}
                            </Link>
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            asChild
                            className="min-h-10 sm:min-h-10"
                          >
                            <Link to={`/projects/${project.id}/webhooks`}>
                              <Webhook className="mr-2 h-4 w-4" aria-hidden />
                              {t("webhooks.title")}
                            </Link>
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            asChild
                            className="min-h-10 sm:min-h-10"
                          >
                            <Link to={`/projects/${project.id}/reports`}>
                              <BarChart3 className="mr-2 h-4 w-4" aria-hidden />
                              {t("reports.title")}
                            </Link>
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            className="min-h-10 sm:min-h-10"
                            onSelect={() => {
                              setShowForm(false);
                              setEditing(project);
                            }}
                          >
                            <Pencil className="mr-2 h-4 w-4" aria-hidden />
                            {t("projects.edit")}
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            className="min-h-10 text-danger focus:text-danger sm:min-h-10"
                            onSelect={() => setPendingDelete(project)}
                          >
                            <Trash2 className="mr-2 h-4 w-4" aria-hidden />
                            {t("common.delete")}
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </div>
                    {editing?.id === project.id && (
                      <form
                        onSubmit={handleUpdate}
                        aria-label={t("projects.editProject", {
                          name: project.name,
                        })}
                        className="grid gap-3 border-t border-border bg-surface-raised p-3 sm:grid-cols-2 lg:grid-cols-3"
                      >
                        <div className="space-y-1.5">
                          <Label htmlFor="edit-project-name">
                            {t("projects.name")}
                          </Label>
                          <Input
                            id="edit-project-name"
                            className="min-h-10"
                            required
                            value={editing.name}
                            onChange={(event) =>
                              setEditing({
                                ...editing,
                                name: event.target.value,
                              })
                            }
                          />
                        </div>
                        <div className="space-y-1.5">
                          <Label htmlFor="edit-project-repo">
                            {t("projects.repositoryUrl")}
                          </Label>
                          <Input
                            id="edit-project-repo"
                            className="min-h-10"
                            required
                            value={editing.repository_url}
                            onChange={(event) =>
                              setEditing({
                                ...editing,
                                repository_url: event.target.value,
                              })
                            }
                          />
                        </div>
                        <div className="space-y-1.5">
                          <Label htmlFor="edit-project-branch">
                            {t("projects.defaultBranch")}
                          </Label>
                          <Input
                            id="edit-project-branch"
                            className="min-h-10"
                            required
                            value={editing.default_branch}
                            onChange={(event) =>
                              setEditing({
                                ...editing,
                                default_branch: event.target.value,
                              })
                            }
                          />
                        </div>
                        <div className="flex flex-wrap justify-end gap-2 sm:col-span-2 lg:col-span-3">
                          <Button
                            type="button"
                            variant="outline"
                            className="min-h-10 sm:min-h-10"
                            disabled={updateProject.isPending}
                            onClick={() => setEditing(null)}
                          >
                            {t("common.cancel")}
                          </Button>
                          <Button
                            type="submit"
                            className="min-h-10 sm:min-h-10"
                            disabled={updateProject.isPending}
                          >
                            {t("common.save")}
                          </Button>
                        </div>
                      </form>
                    )}
                  </li>
                ))}
              </ul>
            )}
            {filteredProjects.length > pageSize && (
              <nav
                aria-label={t("projects.pages")}
                className="flex items-center justify-end gap-2"
              >
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  className="min-h-10 sm:min-h-10"
                  disabled={currentPage === 1}
                  onClick={() => setPage(currentPage - 1)}
                >
                  {t("projects.previous")}
                </Button>
                <span className="text-sm text-text-muted">
                  {currentPage} / {totalPages}
                </span>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  className="min-h-10 sm:min-h-10"
                  disabled={currentPage === totalPages}
                  onClick={() => setPage(currentPage + 1)}
                >
                  {t("projects.next")}
                </Button>
              </nav>
            )}
          </section>
        )}
      </QueryState>
      <ConfirmDialog
        open={pendingDelete !== null}
        title={
          pendingDelete
            ? `${t("projects.deleteConfirm")} "${pendingDelete.name}"?`
            : ""
        }
        closeOnConfirm={false}
        pending={deleteProject.isPending}
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => {
          if (pendingDelete) {
            deleteProject.mutate(pendingDelete.id, {
              onSuccess: () => {
                setPendingDelete(null);
                toast.success(t("projects.deleted"));
              },
              onError: (error) => toast.error(error.message),
            });
          }
        }}
      />
    </div>
  );
}
