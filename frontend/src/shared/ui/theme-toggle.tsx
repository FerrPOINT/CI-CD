import { Moon, CloudSun, Sun } from 'lucide-react'
import { Button } from '@/shared/ui/button'
import { useTheme } from '@/shared/lib/theme'
import { useTranslation } from 'react-i18next'

export function ThemeToggle() {
  const { theme, setTheme } = useTheme()
  const { t } = useTranslation()

  const next: Record<typeof theme, typeof theme> = {
    dark: 'gray',
    gray: 'light',
    light: 'dark',
  }

  return (
    <Button
      variant="ghost"
      size="icon"
      aria-label={t('common.theme', { theme })}
      onClick={() => setTheme(next[theme])}
      className="text-text-secondary hover:text-text-primary hover:bg-surface-raised"
    >
      {theme === 'dark' && <Moon className="h-4 w-4" />}
      {theme === 'gray' && <CloudSun className="h-4 w-4" />}
      {theme === 'light' && <Sun className="h-4 w-4" />}
    </Button>
  )
}
