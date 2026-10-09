import { SidebarItem } from '@sdlc/ui/ui'
import { NamespaceShellContext } from './namespace-context'
import { useProjectDisclosures, WorkspaceNavigation } from './workspace-navigation'
import { useEffect, useState } from 'react'
import { NavLink, Outlet, useLocation } from 'react-router'
import {
  CircleUserRound,
  Cpu,
  FolderGit2,
  GitFork,
  History,
  LayoutDashboard,
  LogOut,
  Menu,
  PanelLeft,
  Settings,
  Users,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import {
  Button,
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  PageFrame,
  PlatformHeader,
  PlatformMark,
  ThemeMenuItems,
} from '@sdlc/ui/ui'
import { useAuth } from '@/shared/auth/auth-provider'

type NavigationListProps = {
  onNavigate?: () => void
  compact?: boolean
  disclosures: ReturnType<typeof useProjectDisclosures>
}

function NavigationList({ onNavigate, compact = false, disclosures }: NavigationListProps) {
  const { t } = useTranslation()
  const location = useLocation()
  const scope = new URLSearchParams(location.search).get('project_scope') === 'all' ? '?project_scope=all' : location.search

  const navItems = [
    { to: '/', icon: LayoutDashboard, label: t('navigation.dashboard') },
    { to: '/projects', icon: FolderGit2, label: t('navigation.projects') },
    { to: '/repositories', icon: GitFork, label: t('navigation.repositories') },
    { to: '/runners', icon: Cpu, label: t('navigation.runners') },
    { to: '/users', icon: Users, label: t('navigation.users') },
    { to: '/audit-log', icon: History, label: t('navigation.auditLog') },
    { to: '/settings', icon: Settings, label: t('navigation.settings') },
  ]

  return (
    <div><nav className="flex flex-col gap-1" aria-label={t('navigation.main')}>
      {navItems.map(({ to, icon: Icon, label }) => (
        <SidebarItem key={to} asChild compact={compact}>
          <NavLink to={to + scope} end={to === '/'} onClick={onNavigate} aria-label={label} title={label}>
            <Icon aria-hidden />
            <span className="base-sidebar-item-label">{label}</span>
          </NavLink>
        </SidebarItem>
      ))}
    </nav>{import.meta.env.VITE_NAMESPACE_ENABLED === 'true' && <WorkspaceNavigation {...disclosures} compact={compact} onNavigate={onNavigate} />}</div>
  )
}

export function AppShell() {
  const { session, logout } = useAuth()
  const { t } = useTranslation()
  const location = useLocation()
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const disclosures = useProjectDisclosures()
  const [compact, setCompact] = useState(() => { try { return localStorage.getItem('forge:sidebar-compact') === 'true' } catch { return false } })
  useEffect(() => { try { localStorage.setItem('forge:sidebar-compact', String(compact)) } catch { /* Storage is optional. */ } }, [compact])
  useEffect(() => {
    if (!window.matchMedia) return
    const media = window.matchMedia('(min-width: 768px)')
    const closeOnDesktop = () => {
      if (media.matches) setMobileMenuOpen(false)
    }
    media.addEventListener('change', closeOnDesktop)
    return () => media.removeEventListener('change', closeOnDesktop)
  }, [])
  const username = session?.username ?? t('navigation.profileFallback')
  const pageLayout =
    location.pathname === '/settings'
      ? 'reading'
      : /^\/repositories\/[^/]+\/pulls\/[^/]+$/.test(location.pathname) &&
          new URLSearchParams(location.search).get('view') !== 'diff'
        ? 'detail-with-aside'
        : 'wide'

  return (
    <div className="min-h-screen bg-background text-text-primary">
      <PlatformHeader
        currentServiceKey="ci-cd"
        context={
          import.meta.env.VITE_NAMESPACE_ENABLED === 'true' ? <NamespaceShellContext /> : undefined
        }
        leading={
          <>
            <Dialog open={mobileMenuOpen} onOpenChange={setMobileMenuOpen}>
              <DialogTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label={t('navigation.toggleMenu')}
                  title={t('navigation.toggleMenu')}
                  className="h-11 w-11 md:hidden"
                >
                  <Menu className="h-5 w-5" aria-hidden />
                </Button>
              </DialogTrigger>
              <DialogContent
                aria-describedby={undefined}
                className="!left-0 !top-0 !flex !h-dvh !max-h-dvh !w-[min(320px,calc(100%-2rem))] !max-w-none !translate-x-0 !translate-y-0 !flex-col !gap-0 !rounded-none !border-y-0 !border-l-0 !p-0 [&>button]:h-11 [&>button]:min-h-11 [&>button]:w-11 [&>button]:min-w-11"
              >
                <DialogHeader className="flex h-[var(--shell-header-height)] shrink-0 justify-center border-b border-border px-4 pr-14 text-left">
                  <DialogTitle className="text-base">
                    <span className="sr-only">{t('navigation.toggleMenu')}</span>
                    <span className="flex items-center gap-3" aria-hidden>
                      <PlatformMark size="sm" withName={false} />
                      <span>{t('app.name')}</span>
                    </span>
                  </DialogTitle>
                </DialogHeader>
                <div className="min-h-0 flex-1 overflow-y-auto p-3" id="mobile-navigation">
                  <NavigationList disclosures={disclosures} onNavigate={() => setMobileMenuOpen(false)} />
                </div>
              </DialogContent>
            </Dialog>

            <NavLink
              to="/"
              className="flex items-center justify-center rounded-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
              aria-label={t('app.name')}
            >
              <PlatformMark size="sm" withName={false} />
            </NavLink>
          </>
        }
        actions={
          <>
            {session && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    aria-label={t('navigation.account')}
                    title={t('navigation.account')}
                  >
                    <CircleUserRound className="h-5 w-5" aria-hidden />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="w-64 max-w-[calc(100vw-2rem)]">
                  <div className="break-words px-2 py-2 text-sm font-medium text-text-primary">
                    {username}
                  </div>
                  <ThemeMenuItems />
                  <DropdownMenuItem
                    onSelect={() => void logout()}
                    className="min-h-11 gap-2 md:min-h-10"
                  >
                    <LogOut className="h-4 w-4" aria-hidden />
                    {t('navigation.logout')}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            )}
          </>
        }
      />
      <aside className={`fixed bottom-0 left-0 top-[var(--shell-header-height)] z-20 hidden ${compact ? 'w-[var(--shell-sidebar-compact)]' : 'w-[var(--shell-sidebar-expanded)]'} flex-col border-r border-border bg-surface md:flex`}>
        <div className="min-h-0 flex-1 overflow-y-auto p-2 xl:p-3">
          <Button variant="ghost" size="icon" title={compact ? 'Развернуть навигацию' : 'Свернуть навигацию'} aria-label={compact ? 'Развернуть навигацию' : 'Свернуть навигацию'} aria-expanded={!compact} onClick={() => setCompact(value => !value)}><PanelLeft aria-hidden size={16} /></Button>
          <NavigationList disclosures={disclosures} compact={compact} />
        </div>
      </aside>
      <div className={`min-w-0 ${compact ? 'md:pl-[var(--shell-sidebar-compact)]' : 'md:pl-[var(--shell-sidebar-expanded)]'}`}>
        <main className="shell-main min-h-[calc(100dvh-var(--shell-header-height))]">
          <PageFrame mode={pageLayout}>
            <Outlet />
          </PageFrame>
        </main>
      </div>
    </div>
  )
}
