import { createRoot } from 'react-dom/client'
import { createBrowserRouter, RouterProvider, useSearchParams } from 'react-router'
import { RepositorySyncPreview } from './preview'
import { ProductionSyncPreview } from './production'
import '../../shared/i18n/config'
import '../../index.css'

function PreviewEntrypoint() {
  const [params] = useSearchParams()
  return params.get('production') === '1' ? (
    <ProductionSyncPreview />
  ) : (
    <RepositorySyncPreview />
  )
}
const router = createBrowserRouter([{ path: '*', element: <PreviewEntrypoint /> }])
createRoot(document.getElementById('root')!).render(<RouterProvider router={router} />)
