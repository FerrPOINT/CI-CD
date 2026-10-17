import { createRoot } from 'react-dom/client'
import { BrowserRouter, useSearchParams } from 'react-router'
import { RepositorySyncPreview } from './preview'
import { ProductionSyncPreview } from './production'
import '../../index.css'

function PreviewEntrypoint() {
  const [params] = useSearchParams()
  return params.get('production') === '1' ? (
    <ProductionSyncPreview />
  ) : (
    <RepositorySyncPreview />
  )
}
createRoot(document.getElementById('root')!).render(
  <BrowserRouter>
    <PreviewEntrypoint />
  </BrowserRouter>,
)
