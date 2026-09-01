import { useEffect, useRef, useState } from 'react'
import { Link, useLocation, Outlet, useNavigate } from 'react-router'
import { GitBranch, LayoutDashboard, FolderGit2, Settings, GitFork, Menu, X, History, Users, Cpu } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/shared/ui/button'
import { ThemeToggle } from '@/shared/ui/theme-toggle'

function NavigationList({ onNavigate, isActive }: { onNavigate: () => void; isActive: (path: string) => boolean }) {
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
    <nav className="flex flex-col gap-1" aria-label={t('navigation.toggleMenu')}>
      {navItems.map(({ to, icon: Icon, label }) => (
        <Link
          key={to}
          to={to}
          onClick={onNavigate}
          aria-current={isActive(to) ? 'page' : undefined}
          className={`flex min-h-11 items-center gap-3 rounded-md px-3 text-sm transition-colors ${
            isActive(to)
              ? 'bg-surface-raised text-text-primary'
              : 'text-text-secondary hover:bg-surface-raised hover:text-text-primary'
          }`}
        >
          <Icon className="h-4 w-4 shrink-0" />
          <span className="break-words">{label}</span>
        </Link>
      ))}
    </nav>
  )
}

export function AppShell() {
  const navigate = useNavigate()
  useEffect(() => {
    let cancelled = false
    import('@/api/auth').then(async (m) => {
      if (cancelled || m.currentSession()) return
      const restored = await m.refresh().catch(() => null)
      if (cancelled || restored || m.currentSession()) return
      const required = await m.authRequired()
      if (required && !cancelled) navigate('/login', { replace: true })
    })
    return () => {
      cancelled = true
    }
  }, [navigate])
  const { t } = useTranslation()
  const location = useLocation()
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const menuButtonRef = useRef<HTMLButtonElement>(null)

  function isActive(path: string) {
    if (path === '/') return location.pathname === '/'
    return location.pathname.startsWith(path)
  }

  // Close drawer on route change
  useEffect(() => {
    setMobileMenuOpen(false)
  }, [location.pathname])

  // Escape closes drawer and restores focus
  useEffect(() => {
    if (!mobileMenuOpen) return
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        setMobileMenuOpen(false)
        menuButtonRef.current?.focus()
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [mobileMenuOpen])

  // Lock body scroll while drawer is open
  useEffect(() => {
    if (!mobileMenuOpen) return
    const prev = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => { document.body.style.overflow = prev }
  }, [mobileMenuOpen])

  return (
    <div className="min-h-screen bg-background text-text-primary">
      <header className="sticky top-0 z-50 flex h-12 items-center justify-between border-b border-border bg-surface px-3 md:px-4">
        <div className="flex items-center gap-3">
          <Button
            ref={menuButtonRef}
            variant="ghost"
            size="icon"
            aria-label={t('navigation.toggleMenu')}
            aria-expanded={mobileMenuOpen}
            aria-controls="mobile-navigation"
            className="h-9 w-9 md:hidden"
            onClick={() => setMobileMenuOpen(v => !v)}
          >
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
        {/* Desktop sidebar */}
        <aside className="hidden w-56 shrink-0 border-r border-border bg-surface p-3 md:block">
          <NavigationList onNavigate={() => {}} isActive={isActive} />
        </aside>

        {/* Mobile drawer */}
        {mobileMenuOpen && (
          <div className="fixed inset-0 z-40 md:hidden" role="dialog" aria-modal="true" aria-label={t('navigation.toggleMenu')}>
            <button
              type="button"
              aria-label={t('common.close')}
              tabIndex={-1}
              className="absolute inset-0 h-full w-full cursor-default bg-black/60"
              onClick={() => setMobileMenuOpen(false)}
            />
            <aside
              id="mobile-navigation"
              className="absolute inset-y-0 left-0 w-64 max-w-[85vw] overflow-y-auto border-r border-border bg-surface p-3 pt-16 shadow-xl"
            >
              <NavigationList onNavigate={() => setMobileMenuOpen(false)} isActive={isActive} />
            </aside>
          </div>
        )}

        <main className="min-w-0 flex-1 p-4 md:p-6">
          <Outlet />
        </main>
      </div>
    </div>
  )
}
