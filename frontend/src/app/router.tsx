import { RequireAdministrator } from "@pdlc/ui/auth";
import { LoadingState } from "@pdlc/ui/ui";
import { createProductRouter, PlatformRoute } from "@pdlc/ui/platform";
import { ConfigurationContext } from "@/widgets/configuration-context";
import { CatalogRepositoryPage } from "@/pages/catalog-repository";
import { CatalogPullPage } from "@/pages/catalog-pull";
import {
  WorkspacePage,
  NamespaceRedirect,
  LegacyConfigRedirect,
} from "@/pages/workspace";
import { DeliveryConfigurationsPage } from "@/pages/delivery-configurations";
import { lazy, Suspense, type ReactElement } from "react";
import { Navigate, Outlet } from "react-router";
import { AppShell } from "@/widgets/app-shell";
import { AuthProvider } from "@/shared/auth/auth-provider";
import { ProtectedRoute } from "@/shared/auth/protected-route";

const RepositorySyncsPage = lazy(() =>
  import("@/pages/repository-syncs").then((m) => ({
    default: m.RepositorySyncsPage,
  })),
);
const RepositorySyncPage = lazy(() =>
  import("@/pages/repository-syncs").then((m) => ({
    default: m.RepositorySyncPage,
  })),
);

const DashboardPage = lazy(() =>
  import("@/pages/dashboard").then((m) => ({ default: m.DashboardPage })),
);
const ProjectsPage = lazy(() =>
  import("@/pages/projects").then((m) => ({ default: m.ProjectsPage })),
);
const PipelinesPage = lazy(() =>
  import("@/pages/pipelines").then((m) => ({ default: m.PipelinesPage })),
);
const PipelineDetailPage = lazy(() =>
  import("@/pages/pipeline-detail").then((m) => ({
    default: m.PipelineDetailPage,
  })),
);
const RepositoriesPage = lazy(() =>
  import("@/pages/repositories").then((m) => ({ default: m.RepositoriesPage })),
);
const RepositoryBrowserPage = lazy(() =>
  import("@/pages/repository-browser").then((m) => ({
    default: m.RepositoryBrowserPage,
  })),
);
const ComparePage = lazy(() =>
  import("@/pages/compare").then((m) => ({ default: m.ComparePage })),
);
const PullRequestsPage = lazy(() =>
  import("@/pages/pull-requests").then((m) => ({
    default: m.PullRequestsPage,
  })),
);
const PullRequestDetailPage = lazy(() =>
  import("@/pages/pull-request-detail").then((m) => ({
    default: m.PullRequestDetailPage,
  })),
);
const SettingsPage = lazy(() =>
  import("@/pages/settings").then((m) => ({ default: m.SettingsPage })),
);
const LoginPage = lazy(() =>
  import("@/pages/login").then((m) => ({ default: m.LoginPage })),
);
const SsoCallbackPage = lazy(() =>
  import("@/pages/sso-callback").then((m) => ({ default: m.SsoCallbackPage })),
);
const ForbiddenPage = lazy(() =>
  import("@/pages/forbidden").then((m) => ({ default: m.ForbiddenPage })),
);
const RunnersPage = lazy(() =>
  import("@/pages/runners").then((m) => ({ default: m.RunnersPage })),
);
const SecretsPage = lazy(() =>
  import("@/pages/secrets").then((m) => ({ default: m.SecretsPage })),
);
const ArtifactsPage = lazy(() =>
  import("@/pages/artifacts").then((m) => ({ default: m.ArtifactsPage })),
);
const EnvironmentsPage = lazy(() =>
  import("@/pages/environments").then((m) => ({ default: m.EnvironmentsPage })),
);
const SchedulesPage = lazy(() =>
  import("@/pages/schedules").then((m) => ({ default: m.SchedulesPage })),
);
const WebhooksPage = lazy(() =>
  import("@/pages/webhooks").then((m) => ({ default: m.WebhooksPage })),
);
const ReportsPage = lazy(() =>
  import("@/pages/reports").then((m) => ({ default: m.ReportsPage })),
);
const AuditLogPage = lazy(() =>
  import("@/pages/audit-log").then((m) => ({ default: m.AuditLogPage })),
);
const UsersPage = lazy(() =>
  import("@/pages/users").then((m) => ({ default: m.UsersPage })),
);

function PageLoader() {
  return <LoadingState />;
}

const withSuspense = (el: ReactElement) => (
  <Suspense fallback={<PageLoader />}>
    <PlatformRoute>{el}</PlatformRoute>
  </Suspense>
);
const ConfigurationSettingsPage = lazy(() =>
  import("@/pages/configuration-settings").then((m) => ({
    default: m.ConfigurationSettingsPage,
  })),
);

export const appRoutes = [
  {
    element: (
      <AuthProvider>
        <Outlet />
      </AuthProvider>
    ),
    children: [
      {
        element: <ProtectedRoute />,
        children: [
          {
            element: <AppShell />,
            children: [
              { path: "/namespace", element: <NamespaceRedirect /> },
              {
                path: "/workspaces/:registry/:namespace/*",
                element: withSuspense(<WorkspacePage />),
              },
              {
                path: "/repository-syncs",
                element: withSuspense(
                  <RequireAdministrator>
                    <RepositorySyncsPage />
                  </RequireAdministrator>,
                ),
              },
              {
                path: "/catalog/repositories/:id/sync",
                element: withSuspense(
                  <RequireAdministrator>
                    <RepositorySyncPage />
                  </RequireAdministrator>,
                ),
              },
              {
                path: "/delivery-configs",
                element: withSuspense(
                  <RequireAdministrator>
                    <DeliveryConfigurationsPage />
                  </RequireAdministrator>,
                ),
              },
              {
                path: "/delivery-configs/:projectId/settings",
                element: withSuspense(
                  <ConfigurationContext>
                    <RequireAdministrator>
                      <ConfigurationSettingsPage />
                    </RequireAdministrator>
                  </ConfigurationContext>,
                ),
              },
              {
                path: "/catalog/repositories/:id",
                element: withSuspense(<CatalogRepositoryPage />),
              },
              {
                path: "/catalog/repositories/:id/pulls/:number",
                element: withSuspense(<CatalogPullPage />),
              },
              { path: "/", element: withSuspense(<DashboardPage />) },
              { path: "/projects", element: withSuspense(<ProjectsPage />) },
              {
                path: "/delivery-configs/:projectId/pipelines",
                element: withSuspense(
                  <ConfigurationContext>
                    <PipelinesPage />
                  </ConfigurationContext>,
                ),
              },
              {
                path: "/pipelines/:pipelineId",
                element: withSuspense(<PipelineDetailPage />),
              },
              {
                path: "/repositories",
                element: withSuspense(<RepositoriesPage />),
              },
              {
                path: "/repositories/:repo",
                element: withSuspense(<RepositoryBrowserPage />),
              },
              {
                path: "/repositories/:repo/compare",
                element: withSuspense(<ComparePage />),
              },
              {
                path: "/repositories/:repo/pulls",
                element: withSuspense(<PullRequestsPage />),
              },
              {
                path: "/repositories/:repo/pulls/:number",
                element: withSuspense(<PullRequestDetailPage />),
              },
              {
                path: "/settings",
                element: withSuspense(
                  <RequireAdministrator>
                    <SettingsPage />
                  </RequireAdministrator>,
                ),
              },
              {
                path: "/runners",
                element: withSuspense(
                  <RequireAdministrator>
                    <RunnersPage />
                  </RequireAdministrator>,
                ),
              },
              {
                path: "/delivery-configs/:projectId/secrets",
                element: withSuspense(
                  <ConfigurationContext>
                    <RequireAdministrator>
                      <SecretsPage />
                    </RequireAdministrator>
                  </ConfigurationContext>,
                ),
              },
              // Preserve old bookmarks; membership mutations are unavailable in Central Auth mode.
              {
                path: "/projects/:projectId/members",
                element: <Navigate to="/projects" replace />,
              },
              {
                path: "/jobs/:jobId/artifacts",
                element: withSuspense(<ArtifactsPage />),
              },
              {
                path: "/delivery-configs/:projectId/environments",
                element: withSuspense(
                  <ConfigurationContext>
                    <RequireAdministrator>
                      <EnvironmentsPage />
                    </RequireAdministrator>
                  </ConfigurationContext>,
                ),
              },
              {
                path: "/delivery-configs/:projectId/schedules",
                element: withSuspense(
                  <ConfigurationContext>
                    <RequireAdministrator>
                      <SchedulesPage />
                    </RequireAdministrator>
                  </ConfigurationContext>,
                ),
              },
              {
                path: "/delivery-configs/:projectId/webhooks",
                element: withSuspense(
                  <ConfigurationContext>
                    <RequireAdministrator>
                      <WebhooksPage />
                    </RequireAdministrator>
                  </ConfigurationContext>,
                ),
              },
              {
                path: "/delivery-configs/:projectId/reports",
                element: withSuspense(
                  <ConfigurationContext>
                    <ReportsPage />
                  </ConfigurationContext>,
                ),
              },
              ...[
                "pipelines",
                "secrets",
                "environments",
                "schedules",
                "webhooks",
                "reports",
              ].map((section) => ({
                path: `/projects/:projectId/${section}`,
                element: <LegacyConfigRedirect />,
              })),
              {
                path: "/audit-log",
                element: withSuspense(
                  <RequireAdministrator>
                    <AuditLogPage />
                  </RequireAdministrator>,
                ),
              },
              {
                path: "/users",
                element: withSuspense(
                  <RequireAdministrator>
                    <UsersPage />
                  </RequireAdministrator>,
                ),
              },
            ],
          },
        ],
      },
      { path: "/login", element: withSuspense(<LoginPage />) },
      { path: "/sso/callback", element: withSuspense(<SsoCallbackPage />) },
    ],
  },
  { path: "/forbidden", element: withSuspense(<ForbiddenPage />) },
  { path: "*", element: <Navigate to="/" replace /> },
];

export const router = createProductRouter(appRoutes, {
  basename: import.meta.env.BASE_URL,
  serviceKey: "ci-cd",
  portalOrigin: import.meta.env.VITE_PLATFORM_ORIGIN,
});
