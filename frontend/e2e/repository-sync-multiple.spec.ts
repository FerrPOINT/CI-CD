import {expect,test} from '@playwright/test'
import type {SyncConnection,SyncRun} from '../src/api/repository-sync'

test.skip(process.env.FORGE_SYNC_HISTORY_FIXTURE!=='1','Explicit isolated fixture only')
const repository='10000000-0000-4000-8000-000000000001'
const github='20000000-0000-4000-8000-000000000001'
const gitlab='20000000-0000-4000-8000-000000000002'
for(const width of [375,768,1440,1920,2560]) test(`two connections keep independent settings and history at ${width}px`,async({page,baseURL},info)=>{
  const origin=new URL(baseURL!).origin
  const connections:SyncConnection[]=[github,gitlab].map((id,index)=>({id,repository_id:repository,repository_name:'platform/service-api',namespace:null,version:1,
    settings:{provider:index?'gitlab':'github',instance_url:index?'https://gitlab.example.test':'https://github.com',repository_path:'fixture/service-api',direction:'both',automatic:!!index},
    has_credential:true,writable:true,paused_reason:null,worker_enabled:true,next_check_at:null,active_run:null,last_run:null}))
  const runs:SyncRun[]=[];const observed:string[]=[]
  await page.route('**/*',async route=>{
    const request=route.request(),url=new URL(request.url())
    if(url.origin!==origin)throw new Error('Fixture attempted external network')
    if(!url.pathname.startsWith('/api/'))return route.continue()
    if(url.pathname==='/api/v1/repository-syncs')return route.fulfill({json:{items:connections,total:2}})
    const selected=url.searchParams.get('connection_id')!
    expect([github,gitlab]).toContain(selected)
    observed.push(selected)
    const connection=connections.find(item=>item.id===selected)!
    if(url.pathname.endsWith('/sync'))return route.fulfill({json:connection})
    expect(url.pathname).toBe(`/api/v1/catalog/repositories/${repository}/sync/runs`)
    if(request.method()==='POST') {
      const run:SyncRun={id:'30000000-0000-4000-8000-000000000001',connection_id:selected,repository_id:repository,connection_version:1,settings:connection.settings,source:'manual',state:'queued',cancel_requested:false,created_at:'2026-10-28T12:00:00Z'}
      runs.push(run);connection.active_run=run
      return route.fulfill({status:202,json:run})
    }
    return route.fulfill({json:{items:runs.filter(run=>run.connection_id===selected),total:runs.filter(run=>run.connection_id===selected).length}})
  })
  await page.setViewportSize({width,height:1000})
  await page.goto('/e2e/fixtures/repository-sync-multiple.html?project_scope=all')
  await expect(page.getByRole('link',{name:'platform/service-api'})).toHaveCount(2)
  expect(await page.getByRole('table',{name:'Подключения синхронизации'}).getByText('GitHub.com',{exact:true}).evaluate(node=>node.closest('td')!.getBoundingClientRect().width)).toBeGreaterThan(180)
  await expect(page.locator(`a[href*="connection_id=${gitlab}"]`).first()).toBeVisible()
  await page.screenshot({path:info.outputPath(`connections-${width}.png`),fullPage:true})
  await page.locator(`a[href*="connection_id=${gitlab}"]`).first().click()
  await expect(page.getByLabel('Адрес инстанса GitLab')).toHaveValue('https://gitlab.example.test')
  await page.getByRole('button',{name:'Синхронизировать сейчас'}).click()
  await expect(page.getByText('Запуск поставлен в очередь')).toBeVisible()
  await page.getByRole('tab',{name:'История запусков',exact:true}).click()
  await expect(page.getByRole('cell',{name:'В очереди',exact:true})).toBeVisible()
  await page.screenshot({path:info.outputPath(`selected-history-${width}.png`),fullPage:true})
  expect(observed.every(id=>id===gitlab)).toBe(true)
  await page.goBack()
  await expect(page.getByRole('tab',{name:'Подключение',exact:true})).toHaveAttribute('aria-selected','true')
  await page.getByRole('link',{name:'Все синхронизации'}).click()
  await page.locator(`a[href*="connection_id=${github}"]`).first().click()
  const history=page.waitForResponse(response=>{
    const url=new URL(response.url())
    return url.pathname.endsWith('/sync/runs')&&url.searchParams.get('connection_id')===github
  })
  await page.getByRole('tab',{name:'История запусков',exact:true}).click()
  await history
  await expect(page.getByRole('cell',{name:'В очереди',exact:true})).toHaveCount(0)
  expect(runs).toHaveLength(1);expect(runs[0].connection_id).toBe(gitlab)
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true)
})
