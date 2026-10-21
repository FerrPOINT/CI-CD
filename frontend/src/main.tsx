import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClientProvider } from '@tanstack/react-query'
import { queryClient } from './shared/query-client'
import { I18nextProvider } from 'react-i18next'
import { RouterProvider } from 'react-router'
import { Toaster } from 'sonner'
import i18n from './shared/i18n/config'
import {
  ThemeProvider,
  useTheme,
  PlatformProvider,
  PlatformServicesProvider,
} from '@pdlc/ui/lib'
import { router } from './app/router'
import './index.css'
import { relaySsoCallback, ssoNamespace } from '@pdlc/ui/sso'
import {
  capturePlatformReturn,
  getPlatformProduct,
  setPlatformQueryClient,
} from '@pdlc/ui/platform'

setPlatformQueryClient(queryClient)
capturePlatformReturn(import.meta.env.VITE_PLATFORM_ORIGIN)

function AppToaster() {
  const { theme } = useTheme()
  return <Toaster theme={theme === 'light' ? 'light' : 'dark'} />
}

if (!relaySsoCallback(ssoNamespace(import.meta.env.VITE_AUTH_NAMESPACE))) {
  const root = createRoot(document.getElementById('root')!)
  getPlatformProduct()?.onRetire(() => {
    root.unmount()
    router.dispose()
  })
  root.render(
    <StrictMode>
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={queryClient}>
          <ThemeProvider>
            <PlatformProvider
              configUrl={import.meta.env.VITE_PLATFORM_BRANDING_URL ?? null}
            >
              <PlatformServicesProvider
                catalogUrl={import.meta.env.VITE_PLATFORM_SERVICES_URL ?? null}
              >
                <RouterProvider router={router} />
              </PlatformServicesProvider>
            </PlatformProvider>
            <AppToaster />
          </ThemeProvider>
        </QueryClientProvider>
      </I18nextProvider>
    </StrictMode>,
  )
}
