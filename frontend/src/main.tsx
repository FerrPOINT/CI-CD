import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { I18nextProvider } from 'react-i18next'
import { RouterProvider } from 'react-router'
import { Toaster } from 'sonner'
import i18n from './shared/i18n/config'
import { ThemeProvider, useTheme, PlatformProvider, PlatformServicesProvider } from '@sdlc/ui/lib'
import { router } from './app/router'
import './index.css'

const queryClient = new QueryClient()

function AppToaster() {
  const { theme } = useTheme()
  return <Toaster theme={theme === 'light' ? 'light' : 'dark'} />
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={queryClient}>
        <ThemeProvider>
          <PlatformProvider configUrl={import.meta.env.VITE_PLATFORM_BRANDING_URL ?? null}>
      <PlatformServicesProvider catalogUrl={import.meta.env.VITE_PLATFORM_SERVICES_URL ?? null}>
            <RouterProvider router={router} />
          </PlatformServicesProvider>
    </PlatformProvider>
          <AppToaster />
        </ThemeProvider>
      </QueryClientProvider>
    </I18nextProvider>
  </StrictMode>,
)
