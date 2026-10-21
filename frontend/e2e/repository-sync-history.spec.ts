import { expect, test } from '@playwright/test'

test.skip(process.env.FORGE_SYNC_HISTORY_FIXTURE !== '1', 'Explicit isolated Vite fixture only; no installed API/SSO')

for (const width of [375, 1026, 1440, 1920, 2560]) {
  test(`one pointer activation is one history entry at ${width}px`, async ({ page, baseURL }, testInfo) => {
    expect(baseURL).toBeTruthy()
    const origin = new URL(baseURL!).origin
    expect(new URL(origin).hostname).toMatch(/^(127\.0\.0\.1|localhost)$/)
    const mutations: string[] = []
    const external: string[] = []
    await page.route('**/*', async route => {
      const request = route.request(), url = new URL(request.url())
      if (url.origin !== origin) {
        external.push(url.origin)
        return route.abort()
      }
      if (url.pathname.startsWith('/api/')) {
        if (request.method() !== 'GET') {
          mutations.push(url.pathname)
          return route.abort()
        }
        expect(url.pathname).toMatch(/^\/api\/v1\/catalog\/repositories\/00000000-0000-4000-8000-000000000001\/sync\/runs$/)
        return route.fulfill({ json: { items: [], total: 0 } })
      }
      return route.continue()
    })
    await page.setViewportSize({ width, height: 1000 })
    await page.goto('/e2e/fixtures/repository-sync-history.html?project_scope=all&provider=github&search=service&tab=connection')
    const connection = page.getByRole('tab', { name: 'Подключение', exact: true })
    const history = page.getByRole('tab', { name: 'История запусков', exact: true })
    await expect(connection).toHaveAttribute('aria-selected', 'true')
    const url = page.url(), index = await page.evaluate(() => window.history.state.idx)
    // A real pointer click executes Radix's nested focus + mousedown callbacks.
    await history.click()
    await expect(history).toHaveAttribute('aria-selected', 'true')
    await expect(page.getByRole('navigation', { name: 'История синхронизации', exact: true })).toBeVisible()
    expect(await page.evaluate(() => window.history.state.idx)).toBe(index + 1)
    await page.goBack()
    await expect(page).toHaveURL(url)
    await expect(connection).toHaveAttribute('aria-selected', 'true')
    await page.goForward()
    await expect(history).toHaveAttribute('aria-selected', 'true')
    expect(new URL(page.url()).searchParams.get('tab')).toBe('history')
    expect(await page.evaluate(() => window.history.state.idx)).toBe(index + 1)
    await page.goBack()
    await expect(connection).toHaveAttribute('aria-selected', 'true')
    await history.click()
    await expect(history).toHaveAttribute('aria-selected', 'true')
    expect(await page.evaluate(() => window.history.state.idx)).toBe(index + 1)
    await history.press('Enter')
    expect(await page.evaluate(() => window.history.state.idx)).toBe(index + 1)
    await history.press('ArrowLeft')
    await expect(connection).toHaveAttribute('aria-selected', 'true')
    expect(await page.evaluate(() => window.history.state.idx)).toBe(index + 2)
    await page.reload()
    await expect(connection).toHaveAttribute('aria-selected', 'true')
    const query = new URL(page.url()).searchParams
    expect(query.get('project_scope')).toBe('all')
    expect(query.get('provider')).toBe('github')
    expect(query.get('search')).toBe('service')
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({ path: testInfo.outputPath(`tabs-${width}.png`), fullPage: true })
    expect(mutations).toEqual([])
    expect(external).toEqual([])
  })
}
