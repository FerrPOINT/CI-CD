import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

test.skip(process.env.SDLC_LIVE_QA !== '1', 'Requires real CI/CD and Central Auth')
test.skip(({ browserName }) => browserName !== 'chromium', 'Single browser acceptance')
test.use({ trace: 'off', video: 'off', screenshot: 'off', hasTouch: true })

const account =
  process.env.SDLC_LIVE_QA === '1'
    ? (JSON.parse(
        readFileSync(
          process.env.SDLC_QA_SESSION_FILE ??
            fileURLToPath(
              new URL('../../../services-base/deploy/.local/qa-session.json', import.meta.url),
            ),
          'utf8',
        ),
      ) as { email: string; password: string })
    : { email: '', password: '' }
const base = process.env.E2E_BASE_URL ?? 'http://localhost:7712'
const evidence =
  process.env.SDLC_HEADER_EVIDENCE_DIR ??
  fileURLToPath(new URL('../../../.local/screenshots/cicd-header/', import.meta.url))

test('CI/CD global header preserves navigation, runtime services, focus and central logout', async ({
  page,
}) => {
  test.setTimeout(600_000)
  mkdirSync(evidence, { recursive: true })
  await page.goto(`${base}/projects`)
  await page.getByLabel('Email').fill(account.email)
  await page.getByLabel('Пароль').fill(account.password)
  await page.getByRole('button', { name: 'Войти', exact: true }).click()
  const switcher = page.getByRole('button', {
    name: 'Открыть список сервисов: CI/CD',
    exact: true,
  })
  await expect(switcher).toBeVisible()
  const errors: string[] = []
  const writes: string[] = []
  const cases: { route: string; theme: string; width: number }[] = []
  page.on('pageerror', (error) => errors.push(error.message))
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(message.text())
  })
  page.on('requestfailed', (request) => {
    if (!request.failure()?.errorText.includes('ERR_ABORTED'))
      errors.push(`${request.method()} ${new URL(request.url()).pathname}`)
  })
  page.on('response', (response) => {
    if (response.status() >= 400 && response.url().includes('/api/v1/'))
      errors.push(`${response.status()} ${new URL(response.url()).pathname}`)
  })
  page.on('request', (request) => {
    if (request.url().includes('/api/v1/') && request.method() !== 'GET')
      writes.push(`${request.method()} ${new URL(request.url()).pathname}`)
  })
  for (const theme of ['light', 'gray', 'dark']) {
    await page.evaluate((value) => localStorage.setItem('theme', value), theme)
    for (const route of ['/projects', '/repositories', '/settings']) {
      await page.goto(`${base}${route}`)
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
      await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible()
      for (const width of [320, 375, 767, 768, 1023, 1024, 1279, 1280, 1440, 1920, 2560]) {
        await page.setViewportSize({
          width,
          height: width >= 1920 ? 1080 : 812,
        })
        const header = page.locator('[data-platform-header]')
        await expect(header).toHaveCount(1)
        const geometry = await header.evaluate((element) => {
          const box = element.getBoundingClientRect()
          return {
            x: box.x,
            right: box.right,
            height: box.height,
            overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
            slots: [...element.querySelectorAll('[data-platform-header-slot]')].map((slot) =>
              slot.getAttribute('data-platform-header-slot'),
            ),
            controls: [...element.querySelectorAll<HTMLElement>('button,a')]
              .filter(
                (control) =>
                  control.getClientRects().length &&
                  getComputedStyle(control).visibility !== 'hidden',
              )
              .map((control) => {
                const rect = control.getBoundingClientRect()
                return {
                  x: rect.x,
                  right: rect.right,
                  width: rect.width,
                  height: rect.height,
                }
              }),
          }
        })
        expect(geometry.x).toBe(0)
        expect(geometry.right).toBe(width)
        expect(geometry.height).toBe(60)
        expect(geometry.slots).toEqual(['leading', 'services', 'actions'])
        expect(geometry.overflow, `${route} ${theme} ${width}px`).toBeLessThanOrEqual(1)
        for (const [index, control] of geometry.controls.entries()) {
          expect(control.width).toBeGreaterThanOrEqual(width < 768 ? 44 : 40)
          expect(control.height).toBeGreaterThanOrEqual(width < 768 ? 44 : 40)
          expect(control.x).toBeGreaterThanOrEqual(0)
          expect(control.right).toBeLessThanOrEqual(width)
          if (index) expect(control.x).toBeGreaterThanOrEqual(geometry.controls[index - 1].right)
        }
        const sidebar = page.locator('aside').first()
        if (width >= 768) {
          await expect(sidebar).toBeVisible()
          const box = await sidebar.boundingBox()
          expect(box?.y).toBe(60)
          expect(box?.width).toBe(width < 1280 ? 72 : 264)
        } else await expect(sidebar).toBeHidden()
        await expect(sidebar).not.toContainText('Forge CI/CD')
        const axe = await new AxeBuilder({ page }).include('[data-platform-header]').analyze()
        expect(
          axe.violations.filter((item) => item.impact === 'serious' || item.impact === 'critical'),
        ).toEqual([])
        if (route === '/projects' && [375, 1920, 2560].includes(width))
          await page.screenshot({
            path: `${evidence}/projects-${theme}-${width}.png`,
            fullPage: true,
            animations: 'disabled',
          })
        cases.push({ route, theme, width })
      }
    }
  }
  for (const width of [375, 2560]) {
    await page.setViewportSize({ width, height: 812 })
    await switcher.focus()
    await page.keyboard.press('Enter')
    const menu = page.getByRole('menu')
    await expect(menu.getByRole('menuitem')).toHaveCount(6)
    await expect(menu.getByRole('img', { name: 'Работает', exact: true })).toHaveCount(6)
    for (const [index, label] of [
      'Admin Panel',
      'CI/CD',
      'Task Tracker',
      'Wiki',
      'Fleet Control',
      'Project Workflow',
    ].entries())
      await expect(menu.getByRole('menuitem').nth(index)).toContainText(label)
    await expect(menu.getByRole('menuitem', { name: /CI\/CD/ })).toHaveAttribute(
      'aria-disabled',
      'true',
    )
    await expect(menu.getByRole('menuitem', { name: /Central Auth|Java Agent|Pulse/ })).toHaveCount(
      0,
    )
    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('Escape')
    await expect(switcher).toBeFocused()
    expect(await page.locator('#root').evaluate((root) => root.inert)).toBe(false)
    await switcher.tap()
    await expect(menu).toBeVisible()
    await page.evaluate(
      () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve())),
    )
    expect(await menu.evaluate((element) => getComputedStyle(element).backgroundColor)).toMatch(
      /^rgb\(|^color\(/,
    )
    const axe = await new AxeBuilder({ page }).analyze()
    expect(
      axe.violations.filter((item) => item.impact === 'serious' || item.impact === 'critical'),
    ).toEqual([])
    await page.screenshot({
      path: `${evidence}/services-menu-dark-${width}.png`,
      fullPage: true,
      animations: 'disabled',
    })
    await page.touchscreen.tap(width - 8, 300)
    await expect(menu).toBeHidden()
    await expect(switcher).toBeFocused()
  }
  await page.setViewportSize({ width: 375, height: 812 })
  const opener = page.getByRole('button', { name: 'Переключить меню' })
  await opener.tap()
  const drawer = page.getByRole('dialog')
  await expect(
    drawer.getByRole('navigation', { name: 'Основная навигация' }).getByRole('link'),
  ).toHaveCount(7)
  const drawerAxe = await new AxeBuilder({ page }).analyze()
  expect(
    drawerAxe.violations.filter((item) => item.impact === 'serious' || item.impact === 'critical'),
  ).toEqual([])
  await page.keyboard.press('Escape')
  await expect(opener).toBeFocused()
  await opener.tap()
  await drawer.getByRole('link', { name: 'Проекты', exact: true }).tap()
  await expect(page).toHaveURL(`${base}/projects`)
  await expect(drawer).toBeHidden()
  await expect(opener).toBeFocused()
  await opener.tap()
  await page.setViewportSize({ width: 1280, height: 800 })
  await expect(drawer).toBeHidden()
  await expect(page.locator('body')).not.toHaveCSS('pointer-events', 'none')
  const profile = page.getByRole('button', { name: 'Аккаунт', exact: true })
  await profile.focus()
  await page.keyboard.press('Enter')
  const menu = page.getByRole('menu')
  const identity = menu.locator('div.break-words')
  await expect(identity).toHaveText(/\S+/)
  const username = (await identity.innerText()).trim()
  await expect(menu.getByText(username, { exact: true })).toHaveCount(1)
  await expect(page.locator('aside')).not.toContainText(username)
  await expect(menu.getByRole('menuitem', { name: 'Выйти', exact: true })).toBeVisible()
  const axe = await new AxeBuilder({ page }).analyze()
  expect(
    axe.violations.filter((item) => item.impact === 'serious' || item.impact === 'critical'),
  ).toEqual([])
  await page.keyboard.press('Escape')
  await expect(profile).toBeFocused()
  expect(await page.locator('#root').evaluate((root) => root.inert)).toBe(false)
  expect(errors).toEqual([])
  expect(writes).toEqual([])
  await profile.click()
  await menu.getByRole('menuitem', { name: 'Выйти', exact: true }).click()
  await expect(page).toHaveURL(/localhost:7701\/oidc\/logout\?client_id=ci-cd/)
  await page.getByRole('button', { name: 'Выйти из всех приложений', exact: true }).click()
  await expect(page).toHaveURL(/localhost:7712\/login\?logged_out/)
  await page.goto(`${base}/projects`, { waitUntil: 'commit' })
  await expect(page).toHaveURL(/localhost:7701\/oidc\/authorize/)
  await expect(page.getByLabel('Пароль')).toBeVisible()
  writeFileSync(
    `${evidence}/results.json`,
    JSON.stringify(
      {
        mock_api: false,
        retries: test.info().retry,
        cases,
        runtime_errors: errors,
        api_writes: writes,
        interactions: [
          'service keyboard/touch/outside/focus',
          'drawer navigation/Escape/desktop resize',
          'profile keyboard/focus',
          'central logout/re-entry',
        ],
      },
      null,
      2,
    ),
  )
})
