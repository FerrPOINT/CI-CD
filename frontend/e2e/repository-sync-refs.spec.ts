import { expect, test } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'
import type { SyncRef, SyncRun } from '../src/api/repository-sync'

test.skip(process.env.FORGE_SYNC_HISTORY_FIXTURE !== '1', 'Explicit isolated Vite fixture only; no installed API/SSO')

const repository = '00000000-0000-4000-8000-000000000001'
const runId = '00000000-0000-4000-8000-000000000002'
const run: SyncRun = {
  id: runId, repository_id: repository, source: 'manual', state: 'success',
  connection_version: 1, cancel_requested: false, created_at: '2026-10-21T10:00:00Z',
  finished_at: '2026-10-21T10:00:01Z',
  settings: { provider: 'github', instance_url: 'https://github.com',
    repository_path: 'fixture/service', direction: 'both', automatic: false },
}
const refs: SyncRef[] = Array.from({ length: 51 }, (_, index) => ({
  ref_name: `refs/heads/qa-${index.toString().padStart(2, '0')}/${'long-component-segment/'.repeat(4)}service-api`,
  old_sha: '1'.repeat(40), new_sha: '2'.repeat(40), direction: 'pull',
  state: 'applied', event_delivered: true,
}))

for (const width of [375, 768, 1026, 1440, 1920, 2560]) {
  test(`long refs remain readable and horizontally accessible at ${width}px`, async ({ page, baseURL }, testInfo) => {
    const origin = new URL(baseURL!).origin
    expect(new URL(origin).hostname).toMatch(/^(127\.0\.0\.1|localhost)$/)
    const mutations: string[] = [], external: string[] = []
    await page.route('**/*', async route => {
      const request = route.request(), url = new URL(request.url())
      if (url.origin !== origin) { external.push(url.origin); return route.abort() }
      if (!url.pathname.startsWith('/api/')) return route.continue()
      if (request.method() !== 'GET') { mutations.push(url.pathname); return route.abort() }
      if (url.pathname === `/api/v1/catalog/repositories/${repository}/sync/runs`) {
        return route.fulfill({ json: { items: [run], total: 1 } })
      }
      expect(url.pathname).toBe(`/api/v1/catalog/repositories/${repository}/sync/runs/${runId}/refs`)
      const offset = Number(url.searchParams.get('offset'))
      expect([0, 50]).toContain(offset)
      expect(url.searchParams.get('limit')).toBe('50')
      return route.fulfill({ json: { items: refs.slice(offset, offset + 50), total: refs.length } })
    })
    await page.setViewportSize({ width, height: 1000 })
    await page.goto(`/e2e/fixtures/repository-sync-history.html?tab=history&run_id=${runId}`)
    const first = page.getByRole('cell', { name: refs[0].ref_name, exact: true })
    await expect(first).toBeVisible()
    await page.evaluate(async () => { await document.fonts.ready; await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))) })
    const geometry = await first.evaluate(cell => ({
      width: cell.getBoundingClientRect().width,
      height: cell.closest('tr')!.getBoundingClientRect().height,
      tableHeight: cell.closest('table')!.getBoundingClientRect().height,
    }))
    await testInfo.attach('ref-geometry', { body: JSON.stringify({ viewportWidth: width, ...geometry }), contentType: 'application/json' })
    // A page-level overflow check alone misses a 20px text column inside a scroll wrapper.
    expect(geometry.width).toBeGreaterThanOrEqual(230)
    expect(geometry.height).toBeLessThanOrEqual(200)
    expect(geometry.tableHeight).toBeLessThan(10_000)
    const table = page.getByRole('table', { name: 'Результаты веток и тегов', exact: true })
    const wrapper = table.locator('..')
    await table.scrollIntoViewIfNeeded()
    await page.getByRole('tab', { name: 'История запусков', exact: true }).focus()
    for (let i = 0; i < 10 && !(await table.evaluate(node => node === document.activeElement)); i++) {
      await page.keyboard.press('Tab')
    }
    await expect(table).toBeFocused()
    const hasOverflow = await wrapper.evaluate(node => node.scrollWidth > node.clientWidth)
    if (width === 375) expect(hasOverflow).toBe(true)
    if (hasOverflow) {
      await page.keyboard.press('ArrowRight')
      await expect.poll(() => wrapper.evaluate(node => node.scrollLeft)).toBeGreaterThan(0)
      await wrapper.evaluate(node => { node.scrollLeft = 0 })
      await table.hover({ position: { x: 100, y: 30 } })
      await page.mouse.wheel(400, 0)
      await expect.poll(() => wrapper.evaluate(node => node.scrollLeft)).toBeGreaterThan(0)
      await wrapper.evaluate(node => { node.scrollLeft = 0 })
    }
    await table.press('ArrowLeft')
    await page.getByRole('heading', { name: 'Результаты веток и тегов', exact: true }).evaluate(node => {
      window.scrollTo(0, scrollY + node.getBoundingClientRect().top - 40)
    })
    await page.screenshot({ path: testInfo.outputPath(`refs-${width}.png`), fullPage: false })
    const pagination = page.getByRole('navigation', { name: 'Результаты веток и тегов', exact: true })
    await pagination.getByRole('button', { name: 'Далее', exact: true }).click()
    await expect(page.getByRole('cell', { name: refs[50].ref_name, exact: true })).toBeVisible()
    await expect(first).toHaveCount(0)
    await table.press('ArrowLeft')
    await page.screenshot({ path: testInfo.outputPath(`refs-last-page-${width}.png`), fullPage: true })
    await expect(pagination.getByRole('button', { name: 'Далее', exact: true })).toBeDisabled()
    await pagination.getByRole('button', { name: 'Назад', exact: true }).click()
    await expect(first).toBeVisible()
    await expect(page.getByRole('cell', { name: refs[50].ref_name, exact: true })).toHaveCount(0)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    if (testInfo.project.name === 'chromium' && [375, 1440].includes(width)) {
      const result = await new AxeBuilder({ page }).include('main').withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze()
      expect(result.violations).toEqual([])
    }
    expect(mutations).toEqual([])
    expect(external).toEqual([])
  })
}
