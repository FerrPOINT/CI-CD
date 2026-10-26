import { useApplicationAccess } from "@pdlc/ui/auth";
import {
  SidebarList,
  AccountMenu,
  AppShell as BaseAppShell,
  SidebarNavigationGroup,
  useSidebarDisclosure,
  NamespaceNavLink as NavLink,
  SidebarItem,
  Button,
  PlatformHeader,
  PlatformMark,
} from "@pdlc/ui/ui";

import { NamespaceShellContext } from "./namespace-context";
import {
  useProjectDisclosures,
  WorkspaceNavigation,
} from "./workspace-navigation";
import { Outlet, useLocation } from "react-router";
import {
  Cpu,
  FolderGit2,
  GitFork,
  RefreshCw,
  History,
  LayoutDashboard,
  Settings,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { useAuth } from "@/shared/auth/auth-provider";

type NavigationListProps = {
  onNavigate?: () => void;
  compact?: boolean;
  disclosures: ReturnType<typeof useProjectDisclosures>;
  utilities: ReturnType<typeof useSidebarDisclosure>;
  utilityActive: boolean;
};

function NavigationList({
  onNavigate,
  compact = false,
  disclosures,
  utilities,
  utilityActive,
}: NavigationListProps) {
  const { t } = useTranslation();
  const location = useLocation();
  const scope =
    new URLSearchParams(location.search).get("project_scope") === "all"
      ? "?project_scope=all"
      : location.search;

  const canAdmin = useApplicationAccess()?.permits([], true) === true;
  const navItems = [
    { to: "/", icon: LayoutDashboard, label: t("navigation.dashboard") },
    { to: "/projects", icon: FolderGit2, label: t("navigation.projects") },
    { to: "/repositories", icon: GitFork, label: "Все репозитории" },
    { to: "/repository-syncs", icon: RefreshCw, label: "Синхронизации" },
  ];
  const utilityItems = [
    { to: "/runners", icon: Cpu, label: t("navigation.runners") },
    { to: "/audit-log", icon: History, label: t("navigation.auditLog") },
    { to: "/settings", icon: Settings, label: "Конфигурация сервера" },
    {
      to: "/delivery-configs",
      icon: FolderGit2,
      label: "Непривязанные CI-конфигурации",
    },
  ];
  const links = (items: typeof navItems) => (
    <SidebarList asChild>
      <nav aria-label={t('navigation.main')}>
        {items.map(({ to, icon: Icon, label }) => (
          <SidebarItem key={to} asChild compact={compact}>
            <NavLink
              to={to + scope}
              end={to === '/'}
              onClick={onNavigate}
              aria-label={label}
              title={label}
            >
              <Icon aria-hidden />
              <span className="base-sidebar-item-label">{label}</span>
            </NavLink>
          </SidebarItem>
        ))}
      </nav>
    </SidebarList>
  )
  return (
    <div>
      {links(
        navItems.filter((item) => canAdmin || item.to !== "/repository-syncs"),
      )}
      {import.meta.env.VITE_NAMESPACE_ENABLED === "true" && (
        <WorkspaceNavigation
          {...disclosures}
          compact={compact}
          onNavigate={onNavigate}
        />
      )}
      {canAdmin && (
        <SidebarNavigationGroup
          label="Служебное"
          active={utilityActive}
          compact={compact}
          {...utilities}
        >
          {links(utilityItems)}
        </SidebarNavigationGroup>
      )}
    </div>
  );
}

export function AppShell() {
  const { session, logout } = useAuth();
  const { t } = useTranslation();
  const location = useLocation();
  const disclosures = useProjectDisclosures();
  const utilityActive = [
    "/runners",
    "/audit-log",
    "/settings",
    "/delivery-configs",
  ].some(
    (to) => location.pathname === to || location.pathname.startsWith(to + "/"),
  );
  const utilities = useSidebarDisclosure(
    utilityActive ? location.pathname : null,
  );
  const username = session?.username ?? t("navigation.profileFallback");
  const pageLayout =
    location.pathname === "/settings"
      ? "reading"
      : /^\/repositories\/[^/]+\/pulls\/[^/]+$/.test(location.pathname) &&
          new URLSearchParams(location.search).get("view") !== "diff"
        ? "detail-with-aside"
        : "wide";

  return (
    <BaseAppShell
      currentServiceKey="ci-cd"
      title={t("app.name")}
      layout={pageLayout}
      collapsibleSidebar
      sidebarStorageKey="forge:sidebar-compact"
      renderNavigation={({ compact, onNavigate }) => (
        <NavigationList
          utilities={utilities}
          utilityActive={utilityActive}
          disclosures={disclosures}
          compact={compact}
          onNavigate={onNavigate}
        />
      )}
      renderHeader={(navigationTrigger) => (
        <PlatformHeader
          currentServiceKey="ci-cd"
          context={
            import.meta.env.VITE_NAMESPACE_ENABLED === "true" ? (
              <NamespaceShellContext />
            ) : undefined
          }
          leading={
            <>
              {navigationTrigger}

              <Button asChild variant="ghost" size="icon">
                <NavLink to="/" aria-label={t("app.name")}>
                  <PlatformMark size="shell" withName={false} />
                </NavLink>
              </Button>
            </>
          }
          actions={
            <>
              {session && (
                <AccountMenu account={{ label: username, onLogout: () => void logout() }} />
              )}
            </>
          }
        />
      )}
    >
      <Outlet />
    </BaseAppShell>
  );
}
