// Evidence screenshots for Forge CI/CD README and docs.
// Usage: node scripts/shoot-evidence.mjs
// Viewport: desktop 1920x1080 full-page.
import { chromium } from 'playwright'
import { mkdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
const OUT = join(ROOT, 'docs', 'screenshots')
mkdirSync(OUT, { recursive: true })

const BASE = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:22802'
const API = process.env.E2E_API_URL ?? 'http://127.0.0.1:22801/api/v1'
const ACCESS_TOKEN = process.env.E2E_ACCESS_TOKEN?.trim()
const LOGIN_EMAIL = process.env.E2E_LOGIN_EMAIL?.trim()
const LOGIN_PASSWORD = process.env.E2E_LOGIN_PASSWORD
const THEME = process.env.EVIDENCE_THEME ?? 'dark'

async function getJson(path) {
  const res = await fetch(`${API}${path}`, {
    headers: ACCESS_TOKEN ? { authorization: `Bearer ${ACCESS_TOKEN}` } : {},
  })
  const text = await res.text()
  if (!res.ok) throw new Error(`GET ${path} -> ${res.status}: ${text.slice(0, 300)}`)
  return text ? JSON.parse(text) : null
}

function pickProject(byName, names) {
  for (const name of names) {
    if (byName[name]) return byName[name]
  }
  throw new Error(`Evidence seed is missing project: expected one of ${names.join(', ')}`)
}

const projects = await getJson('/projects')
const byName = Object.fromEntries(projects.map(p => [p.name, p]))
const platform = pickProject(byName, ['forge-demo-platform', 'platform-core'])
const platformPipes = await getJson(`/projects/${platform.id}/pipelines`)
const pipeline = platformPipes.find(p => p.status === 'success') ?? platformPipes[0]
const failedPipe = platformPipes.find(p => p.status === 'failed')
const pipeDetail = await getJson(`/pipelines/${pipeline.id}`)
const jobId = pipeDetail.stages.flatMap(s => s.jobs)[0]?.id
const pullRequests = await getJson('/repos/platform-core/pulls')
const pullRequest = pullRequests.find(pr => pr.title === 'Add cache layer for registry lookups') ?? pullRequests[0]
if (!pullRequest) throw new Error('Evidence seed is missing a platform-core pull request')
const pullRequestPath = `/repositories/platform-core/pulls/${pullRequest.number}`

const shots = [
  { name: '02-dashboard.png', path: '/', desktop: true, wait: 1200 },
  { name: '03-projects.png', path: '/projects', desktop: true },
  { name: '04-repositories.png', path: '/repositories', desktop: true },
  { name: '05-pipelines.png', path: `/projects/${platform.id}/pipelines`, desktop: true },
  { name: '06-pipeline-detail.png', path: `/pipelines/${pipeline.id}`, desktop: true, wait: 1500 },
  { name: '07-settings.png', path: '/settings', desktop: true },
  { name: '09-repository-browser.png', path: '/repositories/platform-core', desktop: true, wait: 1200, settle: 500 },
  { name: '10-compare.png', path: '/repositories/platform-core/compare?from=main&to=feature%2Fcache-layer', desktop: true, wait: 1200 },
  { name: '11-pull-requests.png', path: '/repositories/platform-core/pulls', desktop: true, wait: 1200 },
  { name: '12-pull-request-detail.png', path: pullRequestPath, desktop: true, wait: 1200 },
  { name: '13-runners.png', path: '/runners', desktop: true },
  { name: '14-secrets.png', path: `/projects/${platform.id}/secrets`, desktop: true },
  { name: '15-environments.png', path: `/projects/${platform.id}/environments`, desktop: true, wait: 1200 },
  { name: '16-schedules.png', path: `/projects/${platform.id}/schedules`, desktop: true },
  { name: '17-webhooks.png', path: `/projects/${platform.id}/webhooks`, desktop: true },
  { name: '18-reports.png', path: `/projects/${platform.id}/reports`, desktop: true, wait: 1200 },
  { name: '19-audit-log.png', path: '/audit-log', desktop: true },
  { name: '20-users.png', path: '/users', desktop: true },
  { name: '21-artifacts.png', path: `/jobs/${jobId}/artifacts`, desktop: true, wait: 800 },
  // --- Состояния действий: диалоги, диффы, логи, формы ---
  { name: '22-pr-diff.png', path: pullRequestPath, desktop: true, wait: 1200, click: 'a[href*="view=diff"]', settle: 1500 },
  { name: '23-project-create.png', path: '/projects', desktop: true, click: 'button:has-text("Создать проект")' },
  {
    name: '24-project-delete-confirm.png',
    path: '/projects',
    desktop: true,
    click: 'button[aria-label^="Действия с проектом"]',
    click2: '[role="menuitem"]:has-text("Удалить")',
    settle: 600,
  },
  { name: '25-repo-create.png', path: '/repositories', desktop: true, click: 'button:has-text("Создать репозиторий")' },
  { name: '26-runner-register.png', path: '/runners', desktop: true, click: 'button:has-text("Зарегистрировать runner")' },
  { name: '27-secret-add.png', path: `/projects/${platform.id}/secrets`, desktop: true, click: 'button:has-text("Добавить секрет")' },
  { name: '28-env-create.png', path: `/projects/${platform.id}/environments`, desktop: true, wait: 800, click: 'button:has-text("Создать окружение")' },
  { name: '29-schedule-create.png', path: `/projects/${platform.id}/schedules`, desktop: true, click: 'button:has-text("Создать расписание")' },
  { name: '30-webhook-add.png', path: `/projects/${platform.id}/webhooks`, desktop: true, click: 'button:has-text("Добавить webhook")' },
  { name: '31-pr-create.png', path: '/repositories/platform-core/pulls', desktop: true, wait: 1000, click: 'button:has-text("Создать pull-запрос")' },
  { name: '33-job-logs.png', path: `/pipelines/${pipeline.id}`, desktop: true, wait: 1500, click: 'button:has-text("Логи")', settle: 1200 },
  { name: '34-pipeline-run-form.png', path: `/projects/${platform.id}/pipelines`, desktop: true, wait: 800, click: 'button:has-text("Запустить пайплайн")', settle: 400 },
  // --- Git-server/CI parity: code, tags, releases, JUnit results ---
  { name: '35-releases-list.png', path: '/repositories/platform-core', desktop: true, wait: 1200, click: 'button:has-text("Релизы")', settle: 500 },
  { name: '36-repo-code-blob.png', path: '/repositories/platform-core', desktop: true, wait: 1200, click: 'button:has-text("Код")', click2: 'button:has-text(".forge-ci.yml")', settle: 700 },
  { name: '37-repo-tags.png', path: '/repositories/platform-core', desktop: true, wait: 1200, click: 'button:has-text("Теги")', settle: 500 },
  { name: '38-releases-create.png', path: '/repositories/platform-core', desktop: true, wait: 1200, click: 'button:has-text("Релизы")', click2: 'button:has-text("Создать релиз")', settle: 500 },
  { name: '39-repo-code-src.png', path: '/repositories/platform-core', desktop: true, wait: 1200, click: 'button:has-text("Код")', click2: 'button:has-text("src")', settle: 500 },
]

const responsiveShots = [
  { name: 'wide.png', path: '/', wait: 1200 },
  { name: 'reading.png', path: '/settings' },
  { name: 'detail-with-aside.png', path: `/pipelines/${pipeline.id}`, wait: 1500 },
]

const readmeShotNames = new Set([
  '02-dashboard.png',
  '03-projects.png',
  '05-pipelines.png',
  '06-pipeline-detail.png',
  '09-repository-browser.png',
  '10-compare.png',
  '11-pull-requests.png',
  '12-pull-request-detail.png',
  '13-runners.png',
  '14-secrets.png',
  '15-environments.png',
  '19-audit-log.png',
  '21-artifacts.png',
  '22-pr-diff.png',
  '24-project-delete-confirm.png',
  '33-job-logs.png',
])
const desktopShots = process.env.EVIDENCE_README_ONLY === 'true'
  ? shots.filter(shot => readmeShotNames.has(shot.name))
  : shots

const browser = await chromium.launch()

async function normalizeVolatileText(page) {
  await page.evaluate(() => {
    if (document.activeElement instanceof HTMLElement) {
      document.activeElement.blur()
    }
    const replacements = [
      [/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi, '00000000-0000-0000-0000-000000000000'],
      [/#([0-9a-f]{8})\b/gi, '#pipeline'],
      [/\b[0-9a-f]{7,40}\b/gi, 'abcdef0'],
      [/\b\d{2}\.\d{2}\.\d{4},\s+\d{2}:\d{2}:\d{2}\b/g, '31.08.2026, 12:00:00'],
      [/\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z\b/g, '2026-08-31T12:00:00Z'],
    ]
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT)
    const nodes = []
    while (walker.nextNode()) nodes.push(walker.currentNode)
    for (const node of nodes) {
      let value = node.nodeValue ?? ''
      for (const [pattern, replacement] of replacements) {
        value = value.replace(pattern, replacement)
      }
      node.nodeValue = value
    }
  })
}

async function openAuthenticated(page, path) {
  await page.goto(BASE + path, { waitUntil: 'domcontentloaded', timeout: 30000 })
  const expectedOrigin = new URL(BASE).origin
  const deadline = Date.now() + 30000
  while (Date.now() < deadline) {
    if (await page.getByLabel('Email').isVisible().catch(() => false)) {
      if (!LOGIN_EMAIL || !LOGIN_PASSWORD) {
        throw new Error('Central Auth login requires E2E_LOGIN_EMAIL and E2E_LOGIN_PASSWORD')
      }
      await page.getByLabel('Email').fill(LOGIN_EMAIL)
      await page.getByLabel('Пароль').fill(LOGIN_PASSWORD)
      await page.getByRole('button', { name: 'Войти', exact: true }).click()
    }
    const current = new URL(page.url())
    if (
      current.origin === expectedOrigin &&
      current.pathname !== '/login' &&
      current.pathname !== '/sso/callback'
    ) {
      return
    }
    await page.waitForTimeout(200)
  }
  throw new Error(`Authentication did not complete for ${path}; current URL: ${page.url()}`)
}

async function shoot(page, shot, outputDir) {
  try {
    await openAuthenticated(page, shot.path)
    await page.addStyleTag({
      content: `
        *, *::before, *::after {
          animation-duration: 0s !important;
          animation-delay: 0s !important;
          transition-duration: 0s !important;
          transition-delay: 0s !important;
          caret-color: transparent !important;
        }
      `,
    })
    await page.locator('main').waitFor({ state: 'visible', timeout: 10000 })
    await page.waitForTimeout(1000)
    if (shot.wait) await page.waitForTimeout(shot.wait)
    if (shot.click) {
      await page.locator(shot.click).first().click({ timeout: 10000 })
      await page.waitForTimeout(shot.settle ?? 800)
    }
    if (shot.click2) {
      await page.locator(shot.click2).first().click({ timeout: 10000 })
      await page.waitForTimeout(shot.settle ?? 800)
    }
    await normalizeVolatileText(page)
    await page.screenshot({ path: join(outputDir, shot.name), fullPage: true })
    console.log('shot', shot.name)
  } catch (err) {
    if (shot.optional) { console.log('skip', shot.name, err.message.slice(0, 80)); return }
    throw err
  }
}

async function captureSet(viewport, outputDir, captureShots) {
  mkdirSync(outputDir, { recursive: true })
  const ctx = await browser.newContext({
    viewport,
    deviceScaleFactor: 1,
    locale: 'ru-RU',
  })
  const page = await ctx.newPage()
  await page.addInitScript((theme) => {
    localStorage.setItem('theme', theme)
    localStorage.setItem('forge.theme', theme)
    localStorage.setItem('i18nextLng', 'ru')
  }, THEME)
  try {
    for (const shot of captureShots) await shoot(page, shot, outputDir)
  } finally {
    await ctx.close()
  }
}

await captureSet({ width: 1920, height: 1080 }, OUT, desktopShots)
await captureSet({ width: 375, height: 812 }, join(OUT, '375x812'), responsiveShots)
await browser.close()
console.log('done:', desktopShots.length + responsiveShots.length)
