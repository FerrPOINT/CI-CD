import { createContext, useContext, useEffect, useState } from 'react'

type Theme = 'dark' | 'gray' | 'light'

interface ThemeContextValue {
  theme: Theme
  setTheme: (theme: Theme) => void
}

const ThemeContext = createContext<ThemeContextValue | null>(null)

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(() => {
    const stored = typeof window !== 'undefined' ? window.localStorage.getItem('theme') : null
    if (stored === 'dark' || stored === 'gray' || stored === 'light') return stored
    return 'dark'
  })

  const setTheme = (value: Theme) => {
    setThemeState(value)
    if (typeof window !== 'undefined') {
      window.localStorage.setItem('theme', value)
    }
  }

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
  }, [theme])

  return <ThemeContext.Provider value={{ theme, setTheme }}>{children}</ThemeContext.Provider>
}

export function useTheme() {
  const ctx = useContext(ThemeContext)
  if (!ctx) throw new Error('useTheme must be used within ThemeProvider')
  return ctx
}
