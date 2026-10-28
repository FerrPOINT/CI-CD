// Isolated production-component fixture; its fictional session never reaches a real API.
import { createRoot } from 'react-dom/client'
import { BrowserRouter, Routes, Route } from 'react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ThemeProvider } from '@pdlc/ui/lib'
import { acceptSso } from '../../src/api/auth'
import { RepositorySyncPage, RepositorySyncsPage } from '../../src/pages/repository-syncs'
import '../../src/index.css'

acceptSso('fictional-isolated-browser-session',Date.now()+3_600_000,'fixture')
const cache=new QueryClient({defaultOptions:{queries:{retry:false}}})
createRoot(document.getElementById('root')!).render(<ThemeProvider theme="dark"><QueryClientProvider client={cache}><BrowserRouter><main className="p-4"><Routes>
  <Route path="/catalog/repositories/:id/sync" element={<RepositorySyncPage/>}/>
  <Route path="*" element={<RepositorySyncsPage/>}/>
</Routes></main></BrowserRouter></QueryClientProvider></ThemeProvider>)
