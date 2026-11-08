// Production components, fictional data; every API request is intercepted here.
import { createRoot } from 'react-dom/client'
import { BrowserRouter, Route, Routes, useNavigate } from 'react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ThemeProvider } from '@pdlc/ui/lib'
import { ActionGroup, Button, Notice, Toaster } from '@pdlc/ui/ui'
import { acceptSso } from '../../src/api/auth'
import { PullComments } from '../../src/pages/pull-request-detail/comments'
import { PullRequestsPage } from '../../src/pages/pull-requests'
import '../../src/shared/i18n/config'
import '../../src/index.css'

const base='a'.repeat(40), head='b'.repeat(40)
const task={tracker_instance_id:'00000000-0000-4000-8000-000000000001',task_id:'00000000-0000-4000-8000-000000000002'}
const comments: Record<string,unknown>[]=[{id:'old',operation_id:'old',author:'Reviewer',
  created_at:'2026-11-08T10:00:00Z',base_sha:base,head_sha:'c'.repeat(40),severity:'P1',
  body:'Сохранённое замечание к удалённой строке.',anchor:{path:'src/removed.rs',side:'old',line:17}}]
const pulls: Record<string,unknown>[]=[]
let loseResponse=true
const originalFetch=window.fetch.bind(window)
window.fetch=async (input,init) => {
  const url=new URL(typeof input==='string'?input:input instanceof URL?input.href:input.url,location.origin)
  if (!url.pathname.startsWith('/api/')) return originalFetch(input,init)
  const method=init?.method??'GET', path=url.pathname
  let data: unknown
  if (path.endsWith('/review-context') && method==='GET') data={base_sha:base,head_sha:head,merge_base:base,pull_request_id:'fixture'}
  else if (path.endsWith('/checks') && method==='GET') data=[{pipeline_id:'fixture-ci',commit_sha:head,status:'success'}]
  else if (path.endsWith('/patch') && method==='GET') return new Response('diff --git a/src/removed.rs b/src/removed.rs\n--- a/src/removed.rs\n+++ b/src/removed.rs\n@@ -17 +17 @@\n-old check\n+new check\n',{headers:{'Content-Type':'text/plain'}})
  else if (path.endsWith('/comments') && method==='GET') data=url.searchParams.has('operation_id')
    ? comments.filter(comment=>comment.operation_id===url.searchParams.get('operation_id')):comments
  else if (path.endsWith('/comments') && method==='POST') {
    const value=JSON.parse(String(init?.body))
    const saved=comments.find(comment=>comment.operation_id===value.operation_id)
    data=saved??{...value,id:crypto.randomUUID(),author:'Reviewer',created_at:new Date().toISOString()}
    if (!saved) comments.push(data as Record<string,unknown>)
    document.getElementById('fixture-count')!.textContent=String(comments.length)
    if (loseResponse) {loseResponse=false;throw new TypeError('Fixture: response lost after acceptance')}
  } else if (path.endsWith('/refs')) data=[{name:'main',kind:'branch',target:base},{name:'feature',kind:'branch',target:head}]
  else if (path.endsWith('/task')) data={task,task_key:'RV-1'}
  else if (path.endsWith('/pulls/page')) data={items:pulls,total:pulls.length,limit:20,offset:0}
  else if (path.endsWith('/pulls') && method==='POST') {
    const value=JSON.parse(String(init?.body))
    if (JSON.stringify(value.primary_task)!==JSON.stringify(task)) return Response.json({error:'Task required'},{status:422})
    data={...value,id:crypto.randomUUID(),number:pulls.length+1,status:'open',created_by:'Reviewer',
      primary_tracker_instance_id:task.tracker_instance_id,primary_task_id:task.task_id,
      created_at:new Date().toISOString(),updated_at:new Date().toISOString(),merged_at:null,merge_commit_sha:null}
    pulls.push(data as Record<string,unknown>)
  } else return Response.json({error:'Unexpected isolated fixture API request'},{status:404})
  return Response.json(data)
}
acceptSso('fictional-isolated-browser-session',Date.now()+3_600_000,'fixture')
function Fixture() {
  const navigate=useNavigate()
  return <main className="mx-auto max-w-5xl space-y-4 p-4">
    <Notice>Изолированная UI-фикстура. Записи остаются в памяти этой страницы.</Notice>
    <ActionGroup><Button onClick={()=>navigate('/e2e/fixtures/reviewer.html')}>Замечания</Button>
      <Button onClick={()=>navigate('/repositories/fixture/pulls')}>Создание PR</Button></ActionGroup>
    <p>Записей в фикстуре: <output id="fixture-count">1</output></p>
    <Routes><Route path="/repositories/:repo/pulls" element={<PullRequestsPage/>}/>
      <Route path="*" element={<PullComments repositoryId="fixture" number={1} repo="fixture"/>}/></Routes>
  </main>
}
const cache=new QueryClient({defaultOptions:{queries:{retry:false}}})
createRoot(document.getElementById('root')!).render(<ThemeProvider theme="dark"><QueryClientProvider client={cache}><BrowserRouter><Fixture/><Toaster/></BrowserRouter></QueryClientProvider></ThemeProvider>)
