import { NamespaceShellContext } from './namespace-context'
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
  responsiveLabels?: boolean
}

function NavigationList({ onNavigate, responsiveLabels = false }: NavigationListProps) {
  const { t } = useTranslation()

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
    <nav className="flex flex-col gap-1" aria-label={t('navigation.main')}>
      {navItems.map(({ to, icon: Icon, label }) => (
        <NavLink
          key={to}
          to={to}
          end={to === '/'}
          onClick={onNavigate}
          title={responsiveLabels ? label : undefined}
          className={({ isActive }) =>
            `flex min-h-11 items-center gap-3 rounded-md px-3 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus md:min-h-10 ${
              responsiveLabels ? 'md:justify-center md:px-2 xl:justify-start xl:px-3' : ''
            } ${
              isActive
                ? 'bg-surface-raised text-text-primary'
                : 'text-text-secondary hover:bg-surface-raised hover:text-text-primary'
            }`
          }
        >
          <Icon className="h-5 w-5 shrink-0" aria-hidden />
          <span
            className={
              responsiveLabels ? 'md:sr-only xl:not-sr-only xl:break-words' : 'break-words'
            }
          >
            {label}
          </span>
        </NavLink>
      ))}
    </nav>
  )
}

export function AppShell() {
  const { session, logout } = useAuth()
  const { t } = useTranslation()
  const location = useLocation()
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
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
                  <NavigationList onNavigate={() => setMobileMenuOpen(false)} />
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
      <aside className="fixed bottom-0 left-0 top-[var(--shell-header-height)] z-20 hidden w-[var(--shell-sidebar-compact)] flex-col border-r border-border bg-surface md:flex xl:w-[var(--shell-sidebar-expanded)]">
        <div className="min-h-0 flex-1 overflow-y-auto p-2 xl:p-3">
          <NavigationList responsiveLabels />
        </div>
      </aside>
      <div className="min-w-0 md:pl-[var(--shell-sidebar-compact)] xl:pl-[var(--shell-sidebar-expanded)]">
        <main className="shell-main min-h-[calc(100dvh-var(--shell-header-height))]">
          <PageFrame mode={pageLayout}>
            <Outlet />
          </PageFrame>
        </main>
      </div>
    </div>
  )
}
