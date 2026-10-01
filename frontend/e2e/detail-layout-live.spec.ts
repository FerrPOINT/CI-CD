import { execFileSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

test.skip(process.env.SDLC_LIVE_QA !== '1', 'Requires live CI/CD and Central Auth')
test.skip(({ browserName }) => browserName !== 'chromium', 'Single browser live acceptance')
test.use({ trace: 'off', video: 'off', screenshot: 'off', hasTouch: true })

const account = process.env.SDLC_LIVE_QA === '1'
  ? JSON.parse(readFileSync(process.env.SDLC_QA_SESSION_FILE ?? fileURLToPath(
    new URL('../../../services-base/deploy/.local/qa-session.json', import.meta.url),
  ), 'utf8')) as { email: string; password: string }
  : { email: '', password: '' }
const base = process.env.E2E_BASE_URL ?? 'http://localhost:7712'
const apiBase = process.env.E2E_API_URL ?? 'http://localhost:7711/api/v1'
const authBase = process.env.SDLC_AUTH_URL ?? 'http://localhost:7701'
const screenshots = fileURLToPath(new URL('../../../.local/screenshots/cicd-detail-layout/', import.meta.url))

function removeOwnWorkspace(work: string) {
  const ownPath = resolve(work)
  if (dirname(ownPath) !== resolve(tmpdir()) || !ownPath.includes('sdlc-cicd-detail-'))
    throw new Error('Unsafe QA workspace cleanup path')
  rmSync(ownPath, { recursive: true, force: true })
}

test('CI/CD uses a real PR actions rail and wide pipeline/diff working areas', async ({ page, request }) => {
  test.setTimeout(900_000)
  mkdirSync(screenshots, { recursive: true })
  const login = await request.post(`${authBase}/auth/login`, {
    data: { email: account.email, password: account.password },
  })
  expect(login.ok()).toBeTruthy()
  const { access_token } = await login.json() as { access_token: string }
  const headers = { Authorization: `Bearer ${access_token}` }
  const repo = `qa-layout-${Date.now().toString(36)}`
  const work = mkdtempSync(join(tmpdir(), 'sdlc-cicd-detail-'))
  let repositoryCreated = false
  let projectId = ''
  const api = async (method: string, path: string, data?: unknown) => {
    const response = await request.fetch(`${apiBase}${path}`, { method, headers, data })
    expect(response.ok(), `${method} ${path} -> ${response.status()}`).toBeTruthy()
    return response
  }
  // Git receives the credential through its environment, never URL or argv.
  const git = (...args: string[]) => execFileSync('git', args, {
    cwd: work,
    encoding: 'utf8',
    env: {
      ...process.env,
      GIT_TERMINAL_PROMPT: '0',
      GIT_CONFIG_COUNT: '2',
      GIT_CONFIG_KEY_0: 'credential.helper',
      GIT_CONFIG_VALUE_0: '',
      GIT_CONFIG_KEY_1: 'http.extraHeader',
      GIT_CONFIG_VALUE_1: `Authorization: Bearer ${access_token}`,
    },
  })
  try {
    const created = await api('POST', '/repositories', { name: repo, visibility: 'private' })
    repositoryCreated = created.ok()
    git('init', '-b', 'main')
    git('config', 'user.name', 'SDLC QA')
    git('config', 'user.email', 'qa@example.test')
    writeFileSync(join(work, 'README.md'), '# QA detail layout\n')
    writeFileSync(join(work, '.forge-ci.yml'), [
      'version: 1', 'defaults:', '  image: alpine:3.21', 'jobs:',
      '  compile:', '    commands:', '      - "printf SDLC_QA_DETAIL_COMPILE"',
      '  tests:', '    needs: [compile]', '    commands:', '      - "printf SDLC_QA_DETAIL_TESTS"',
      '  package:', '    needs: [tests]', '    commands:', '      - "printf SDLC_QA_DETAIL_PACKAGE"', '',
    ].join('\n'))
    git('add', '.')
    git('commit', '-m', 'QA safe detail fixture')
    git('checkout', '-b', 'qa/layout')
    writeFileSync(join(work, 'README.md'), '# QA detail layout\n\nSafe review fixture.\n')
    git('add', 'README.md')
    git('commit', '-m', 'QA review change')
    git('push', `${apiBase.replace(/\/api\/v1\/?$/, '')}/git/${repo}.git`, 'main', 'qa/layout')

    const project = await (await api('POST', '/projects', {
      name: `QA ${repo}`, repository_url: `${apiBase.replace(/\/api\/v1\/?$/, '')}/git/${repo}.git`, default_branch: 'main',
    })).json() as { id: string }
    projectId = project.id
    const triggered = await (await api('POST', `/projects/${projectId}/pipelines`, {
      git_ref: 'main',
    })).json() as { pipeline: { id: string } }
    const pipeline = triggered.pipeline
    expect(pipeline.id).toMatch(/^[0-9a-f-]{36}$/)
    let detail: { pipeline: { status: string }; stages: { jobs: { id: string; status: string }[] }[] } | undefined
    await expect.poll(async () => {
      detail = await (await api('GET', `/pipelines/${pipeline.id}`)).json()
      return detail?.pipeline.status
    }, { timeout: 180_000, intervals: [1000, 2000] }).toBe('success')
    expect(detail!.stages).toHaveLength(3)
    for (const stage of detail!.stages) {
      expect(stage.jobs[0].status).toBe('success')
      const logs = await (await api('GET', `/jobs/${stage.jobs[0].id}/logs`)).json()
      expect(JSON.stringify(logs)).toContain('SDLC_QA_DETAIL_')
    }
    const pr = await (await api('POST', `/repos/${repo}/pulls`, {
      repository_name: repo, title: 'QA безопасная проверка геометрии',
      description: 'Собственный внутренний QA PR. Изменения не сливаются.',
      source_branch: 'qa/layout', target_branch: 'main',
    })).json() as { number: number }

    await page.goto(`${base}/repositories/${repo}/pulls/${pr.number}`)
    await page.getByLabel('Email').fill(account.email)
    await page.getByLabel('Пароль').fill(account.password)
    await page.getByRole('button', { name: 'Войти', exact: true }).click()
    await expect(page.getByRole('complementary', { name: 'Действия' })).toBeVisible()
    const errors: string[] = []
    const mutations: string[] = []
    page.on('pageerror', (error) => errors.push(`page: ${error.message}`))
    page.on('console', (message) => { if (message.type() === 'error') errors.push(`console: ${message.text()}`) })
    page.on('requestfailed', (failed) => {
      const reason = failed.failure()?.errorText ?? 'unknown'
      if (!reason.includes('ERR_ABORTED')) errors.push(`${new URL(failed.url()).pathname}: ${reason}`)
    })
    page.on('response', (response) => {
      if (response.url().includes('/api/v1/') && response.status() >= 400)
        errors.push(`${response.status()} ${new URL(response.url()).pathname}`)
    })
    page.on('request', (outgoing) => {
      if (outgoing.url().includes('/api/v1/') && outgoing.method() !== 'GET')
        mutations.push(`${outgoing.method()} ${new URL(outgoing.url()).pathname}`)
    })
    const routes = [
      ['pull', `/repositories/${repo}/pulls/${pr.number}`, 'detail-with-aside'],
      ['diff', `/repositories/${repo}/pulls/${pr.number}?view=diff`, 'wide'],
      ['pipeline', `/pipelines/${pipeline.id}`, 'wide'],
      ['logs', `/pipelines/${pipeline.id}`, 'wide'],
    ] as const
    for (const theme of ['light', 'gray', 'dark']) {
      await page.evaluate((value) => localStorage.setItem('theme', value), theme)
      for (const [name, path, mode] of routes) {
        await page.goto(`${base}${path}`)
        await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
        await expect(page.getByRole('heading', { level: 1 })).toBeVisible()
        const frame = page.locator('main [data-page-layout]').first()
        await expect(frame).toHaveAttribute('data-page-layout', mode)
        if (name === 'diff') await expect(page.getByText('Safe review fixture.')).toBeVisible()
        if (name === 'logs') {
          await page.getByRole('button', { name: /Логи/ }).first().click()
          await expect(page.getByRole('log')).toContainText('SDLC_QA_DETAIL_')
        }
        for (const width of [375, 768, 1023, 1024, 1279, 1280, 1440, 1920, 2560]) {
          await page.setViewportSize({ width, height: width < 768 ? 812 : 1080 })
          await page.evaluate(() => window.scrollTo(0, 0))
          if (name === 'pull') {
            const split = page.locator('.page-split')
            const boxes = await split.evaluate((element) => {
              const a = element.firstElementChild!.getBoundingClientRect()
              const b = element.lastElementChild!.getBoundingClientRect()
              return { a: { x: a.x, y: a.y, right: a.right, bottom: a.bottom, width: a.width }, b: { x: b.x, y: b.y, width: b.width }, gap: parseFloat(getComputedStyle(element).columnGap) }
            })
            if (width >= 1024) {
              expect(boxes.b.width).toBeCloseTo(320, 0)
              expect(boxes.b.x - boxes.a.right).toBeCloseTo(boxes.gap, 0)
              expect(boxes.b.y).toBeCloseTo(boxes.a.y, 0)
            } else {
              expect(boxes.b.x).toBeCloseTo(boxes.a.x, 0)
              expect(boxes.b.width).toBeCloseTo(boxes.a.width, 0)
              expect(boxes.b.y).toBeGreaterThanOrEqual(boxes.a.bottom)
            }
          } else {
            await expect(page.locator('.page-split')).toHaveCount(0)
            if (name === 'pipeline' || name === 'logs') {
              const grid = await page.locator('[data-pipeline-layout="stage-grid"]').evaluate((element) => ({
                gap: parseFloat(getComputedStyle(element).gap),
                boxes: Array.from(element.children).map((item) => {
                  const box = item.getBoundingClientRect()
                  return { x: box.x, y: box.y, right: box.right, bottom: box.bottom, width: box.width }
                }),
              }))
              const widths = grid.boxes.map((box) => box.width)
              expect(widths).toHaveLength(3)
              expect(Math.max(...widths) - Math.min(...widths)).toBeLessThanOrEqual(1)
              for (let index = 1; index < grid.boxes.length; index++) {
                if (width >= 1024) {
                  expect(grid.boxes[index].y).toBeCloseTo(grid.boxes[0].y, 0)
                  expect(grid.boxes[index].x - grid.boxes[index - 1].right).toBeCloseTo(grid.gap, 0)
                } else {
                  expect(grid.boxes[index].x).toBeCloseTo(grid.boxes[0].x, 0)
                  expect(grid.boxes[index].y).toBeGreaterThanOrEqual(grid.boxes[index - 1].bottom)
                }
              }
              if (width >= 1920) expect(Math.min(...widths)).toBeGreaterThan(320)
            }
          }
          expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth), `${name} ${theme} ${width} overflow`).toBeLessThanOrEqual(1)
          const audit = await new AxeBuilder({ page }).analyze()
          expect(audit.violations.filter((item) => item.impact === 'serious' || item.impact === 'critical').map((item) => ({ id: item.id, targets: item.nodes.map((node) => node.target) })), `${name} ${theme} ${width} axe`).toEqual([])
          await page.screenshot({ path: `${screenshots}/${name}-${theme}-${width}.png`, fullPage: true, animations: 'disabled' })
        }
        if (name === 'pull') {
          for (const width of [375, 2560]) {
            await page.setViewportSize({ width, height: 812 })
            for (const action of ['Закрыть', 'Слить']) {
              const trigger = page.getByRole('button', { name: action, exact: true })
              expect((await trigger.boundingBox())!.height, `${action} ${theme} ${width} trigger`).toBeGreaterThanOrEqual(40)
              if (width === 375) await trigger.tap()
              else { await trigger.focus(); await page.keyboard.press('Enter') }
              const dialog = page.getByRole('alertdialog')
              await expect(dialog).toContainText('qa/layout')
              for (const button of await dialog.getByRole('button').all()) {
                const box = await button.boundingBox()
                expect(box!.height, `${action} ${theme} ${width} dialog button`).toBeGreaterThanOrEqual(40)
              }
              const modalAudit = await new AxeBuilder({ page }).analyze()
              expect(modalAudit.violations.filter((item) => item.impact === 'serious' || item.impact === 'critical').map((item) => item.id), `${action} ${theme} ${width} dialog axe`).toEqual([])
              await page.keyboard.press('Escape')
              await expect(dialog).toBeHidden()
              await expect(trigger).toBeFocused()
            }
          }
        }
      }
    }
    expect(mutations, 'Reading pipeline/logs/PR/diff and cancelling confirmation must not mutate').toEqual([])
    expect(errors).toEqual([])
  } finally {
    try {
      if (projectId) await api('DELETE', `/projects/${projectId}`)
    } finally {
      try {
        if (repositoryCreated) await api('DELETE', `/repositories/${repo}`)
      } finally {
        removeOwnWorkspace(work)
      }
    }
  }
})
