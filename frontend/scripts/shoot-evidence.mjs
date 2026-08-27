// Evidence screenshots for Forge CI/CD README and docs.
// Usage: node scripts/shoot-evidence.mjs
// Viewports: desktop 1920x1080 full-page, mobile 375x812.
import { chromium } from 'playwright'
import { mkdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
const OUT = join(ROOT, 'docs', 'screenshots')
mkdirSync(OUT, { recursive: true })

const BASE = 'http://127.0.0.1:22802'
const API = 'http://127.0.0.1:22801/api/v1'

const projects = await (await fetch(`${API}/projects`)).json()
const byName = Object.fromEntries(projects.map(p => [p.name, p]))
const platform = byName['forge-demo-platform']
const web = byName['forge-demo-web']
const platformPipes = await (await fetch(`${API}/projects/${platform.id}/pipelines`)).json()
const pipeline = platformPipes.find(p => p.status === 'success') ?? platformPipes[0]
const failedPipe = platformPipes.find(p => p.status === 'failed')
const pipeDetail = await (await fetch(`${API}/pipelines/${pipeline.id}`)).json()
const jobId = pipeDetail.stages.flatMap(s => s.jobs)[0]?.id

const shots = [
  { name: '01-login.png', path: '/login', desktop: true },
  { name: '02-dashboard.png', path: '/', desktop: true, wait: 1200 },
  { name: '03-projects.png', path: '/projects', desktop: true },
  { name: '04-repositories.png', path: '/repositories', desktop: true },
  { name: '05-pipelines.png', path: `/projects/${platform.id}/pipelines`, desktop: true },
  { name: '06-pipeline-detail.png', path: `/pipelines/${pipeline.id}`, desktop: true, wait: 1500 },
  { name: '07-settings.png', path: '/settings', desktop: true },
  { name: '08-admin.png', path: '/admin', desktop: true },
  { name: '09-repository-browser.png', path: '/repositories/platform-core', desktop: true, wait: 1200 },
  { name: '10-compare.png', path: '/repositories/platform-core/compare?from=main&to=feature%2Fcache-layer', desktop: true, wait: 1200 },
  { name: '11-pull-requests.png', path: '/repositories/platform-core/pulls', desktop: true, wait: 1200 },
  { name: '12-pull-request-detail.png', path: '/repositories/platform-core/pulls/8', desktop: true, wait: 1200 },
  { name: '13-runners.png', path: '/runners', desktop: true },
  { name: '14-secrets.png', path: `/projects/${platform.id}/secrets`, desktop: true },
  { name: '15-environments.png', path: `/projects/${platform.id}/environments`, desktop: true, wait: 1200 },
  { name: '16-schedules.png', path: `/projects/${platform.id}/schedules`, desktop: true },
  { name: '17-webhooks.png', path: `/projects/${platform.id}/webhooks`, desktop: true },
  { name: '18-reports.png', path: `/projects/${platform.id}/reports`, desktop: true, wait: 1200 },
  { name: '19-audit-log.png', path: '/audit-log', desktop: true },
  { name: '20-users.png', path: '/users', desktop: true },
  { name: '21-artifacts.png', path: `/jobs/${jobId}/artifacts`, desktop: true, wait: 800 },
  { name: 'm-dashboard.png', path: '/', mobile: true, wait: 1200 },
  { name: 'm-projects.png', path: '/projects', mobile: true },
  { name: 'm-pipeline-detail.png', path: `/pipelines/${pipeline.id}`, mobile: true, wait: 1500 },
  { name: 'm-runners.png', path: '/runners', mobile: true },
  { name: 'm-pull-request.png', path: '/repositories/platform-core/pulls/8', mobile: true, wait: 1200 },
]

const browser = await chromium.launch()
async function shoot(shot) {
  const ctx = await browser.newContext({
    viewport: shot.mobile ? { width: 375, height: 812 } : { width: 1920, height: 1080 },
    deviceScaleFactor: 1,
    locale: 'ru-RU',
  })
  const page = await ctx.newPage()
  await page.addInitScript(() => {
    localStorage.setItem('forge.theme', 'dark')
    localStorage.setItem('i18nextLng', 'ru')
  })
  try {
    await page.goto(BASE + shot.path, { waitUntil: 'networkidle', timeout: 30000 })
    if (shot.wait) await page.waitForTimeout(shot.wait)
    await page.screenshot({ path: join(OUT, shot.name), fullPage: true })
    console.log('shot', shot.name)
  } catch (err) {
    if (shot.optional) { console.log('skip', shot.name, err.message.slice(0, 80)); return }
    throw err
  } finally {
    await ctx.close()
  }
}
for (const shot of shots) await shoot(shot)
await browser.close()
console.log('done:', shots.length)
