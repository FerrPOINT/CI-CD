import { useState } from 'react'
import { NavLink, Outlet } from 'react-router'
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
  PlatformMark,
  ServiceSwitcher,
  ThemeToggle,
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
          className={({ isActive }) => `flex min-h-11 items-center gap-3 rounded-md px-3 text-sm transition-colors ${
            responsiveLabels ? 'md:justify-center md:px-2 xl:justify-start xl:px-3' : ''
          } ${
            isActive
              ? 'bg-surface-raised text-text-primary'
              : 'text-text-secondary hover:bg-surface-raised hover:text-text-primary'
          }`}
        >
          <Icon className="h-5 w-5 shrink-0" aria-hidden />
          <span className={responsiveLabels ? 'md:sr-only xl:not-sr-only xl:break-words' : 'break-words'}>{label}</span>
        </NavLink>
      ))}
    </nav>
  )
}

export function AppShell() {
  const { session, logout } = useAuth()
  const { t } = useTranslation()
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const username = session?.username ?? t('navigation.profileFallback')

  return (
    <div className="min-h-screen bg-background text-text-primary">
      <aside className="fixed inset-y-0 left-0 z-40 hidden w-[72px] flex-col border-r border-border bg-surface md:flex xl:w-[264px]">
        <div className="flex h-[60px] shrink-0 items-center justify-center border-b border-border px-3 xl:justify-start xl:px-5">
          <div className="flex min-w-0 items-center gap-3" aria-label={t('app.name')}>
            <PlatformMark size="sm" withName={false} />
            <div className="hidden min-w-0 xl:block">
              <div className="truncate text-sm font-semibold">{t('app.name')}</div>
              <div className="truncate text-xs text-text-muted">{t('app.subtitle')}</div>
            </div>
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-2 xl:p-3">
          <NavigationList responsiveLabels />
        </div>
      </aside>

      <div className="min-h-screen md:pl-[72px] xl:pl-[264px]">
        <header className="sticky top-0 z-30 flex h-[60px] items-center justify-between border-b border-border bg-surface px-3 md:px-4">
          <div className="flex min-w-0 items-center gap-2">
            <Dialog open={mobileMenuOpen} onOpenChange={setMobileMenuOpen}>
              <DialogTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label={t('navigation.toggleMenu')}
                  className="h-11 w-11 md:hidden"
                >
                  <Menu className="h-5 w-5" aria-hidden />
                </Button>
              </DialogTrigger>
              <DialogContent
                aria-describedby={undefined}
                className="!left-0 !top-0 !flex !h-dvh !max-h-dvh !w-[min(320px,calc(100%-2rem))] !max-w-none !translate-x-0 !translate-y-0 !flex-col !gap-0 !rounded-none !border-y-0 !border-l-0 !p-0 [&>button]:h-10 [&>button]:w-10"
              >
                <DialogHeader className="flex h-[60px] shrink-0 justify-center border-b border-border px-4 pr-14 text-left">
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

            <div className="flex min-w-0 items-center gap-2 md:hidden" aria-label={t('app.name')}>
              <PlatformMark size="sm" withName={false} />
              <span className="hidden truncate text-sm font-semibold min-[420px]:inline">{t('app.name')}</span>
            </div>
            <span className="hidden truncate text-sm font-medium text-text-secondary md:inline">
              {t('app.subtitle')}
            </span>
          </div>

          <div className="flex shrink-0 items-center gap-1 sm:gap-2">
            <ServiceSwitcher currentKey="ci-cd" />
            <div className="[&_button]:h-10 [&_button]:w-10">
              <ThemeToggle />
            </div>
            {session && (
              <div className="flex h-10 items-center" title={username} aria-label={username}>
                <CircleUserRound className="h-5 w-5 text-text-muted" aria-hidden />
                <span className="ml-2 hidden max-w-36 truncate text-sm text-text-secondary xl:inline">{username}</span>
              </div>
            )}
            {session && (
              <Button
                variant="ghost"
                size="sm"
                aria-label={t('navigation.logout')}
                className="h-10 min-w-10 px-2 sm:px-3"
                onClick={() => void logout()}
              >
                <LogOut className="h-4 w-4" aria-hidden />
                <span className="hidden sm:inline">{t('navigation.logout')}</span>
              </Button>
            )}
          </div>
        </header>

        <main className="min-w-0 p-4 md:p-6">
          <Outlet />
        </main>
      </div>
    </div>
  )
}
