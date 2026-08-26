import { useState } from 'react'
import { Link, useLocation, Outlet } from 'react-router'
import { GitBranch, LayoutDashboard, FolderGit2, Settings, GitFork, Server, Menu, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/shared/ui/button'
import { ThemeToggle } from '@/shared/ui/theme-toggle'

export function AppShell() {
  const { t } = useTranslation()
  const location = useLocation()
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)

  const navItems = [
    { to: '/', icon: LayoutDashboard, label: t('navigation.dashboard') },
    { to: '/projects', icon: FolderGit2, label: t('navigation.projects') },
    { to: '/repositories', icon: GitFork, label: t('navigation.repositories') },
    { to: '/admin', icon: Server, label: t('navigation.admin') },
    { to: '/settings', icon: Settings, label: t('navigation.settings') },
  ]

  function isActive(path: string) {
    if (path === '/') return location.pathname === '/'
    return location.pathname.startsWith(path)
  }

  return (
    <div className="min-h-screen bg-background text-text-primary">
      <header className="sticky top-0 z-50 flex h-12 items-center justify-between border-b border-border bg-surface px-3 md:px-4">
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="icon" className="h-8 w-8 md:hidden" onClick={() => setMobileMenuOpen(v => !v)}>
            {mobileMenuOpen ? <X className="h-[18px] w-[18px]" /> : <Menu className="h-[18px] w-[18px]" />}
          </Button>
          <Link to="/" className="flex items-center gap-2 font-bold">
            <GitBranch className="h-[18px] w-[18px] text-accent" />
            <span className="hidden sm:inline">{t('app.name')}</span>
          </Link>
        </div>
        <div className="flex items-center gap-2">
          <ThemeToggle />
        </div>
      </header>

      <div className="flex">
        <aside className={`${mobileMenuOpen ? 'block' : 'hidden md:block'} w-56 shrink-0 border-r border-border bg-surface p-3`}>
          <nav className="flex flex-col gap-1">
            {navItems.map(({ to, icon: Icon, label }) => (
              <Link
                key={to}
                to={to}
                onClick={() => setMobileMenuOpen(false)}
                className={`flex items-center gap-3 rounded-md px-3 py-2 text-sm transition-colors ${
                  isActive(to)
                    ? 'bg-surface-raised text-text-primary'
                    : 'text-text-secondary hover:bg-surface-raised hover:text-text-primary'
                }`}
              >
                <Icon className="h-4 w-4 shrink-0" />
                <span className="truncate">{label}</span>
              </Link>
            ))}
          </nav>
        </aside>

        <main className="flex-1 overflow-x-auto p-4 md:p-6">
          <Outlet />
        </main>
      </div>
    </div>
  )
}
