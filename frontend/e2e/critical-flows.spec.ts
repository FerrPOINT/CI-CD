import { expect, test } from '@playwright/test'
import { expectedArtifactName, waitForEvidence } from './evidence'

test.describe('critical dashboard journeys', () => {
  test('[REQ-UI-001] opens seeded project, pipeline plan, logs and artifacts on the built app', async ({ page, request }) => {
    const evidence = await waitForEvidence(request)

    await page.goto('/')
    await expect(page.getByRole('heading', { name: 'Дашборд' })).toBeVisible()
    await expect(page.getByText('forge-demo-platform')).toBeVisible()

    await page.getByRole('link', { name: /forge-demo-platform/ }).first().click()
    await expect(page.getByRole('heading', { name: 'Пайплайны' })).toBeVisible()
    await expect(page.getByText('main').first()).toBeVisible()

    await page.goto(`/pipelines/${evidence.pipeline.id}`)
    await expect(page.getByRole('heading', { name: `#${evidence.pipeline.id.slice(0, 8)}` })).toBeVisible()
    await expect(page.getByText('План запуска')).toBeVisible()
    await expect(page.getByText(evidence.job.name).first()).toBeVisible()
    await expect(page.getByText('target/release/app.tar.gz', { exact: true })).toBeVisible()

    await page.getByRole('button', { name: /Логи/ }).first().click()
    await expect(page.getByText(new RegExp(`Логи.*${evidence.job.name}`))).toBeVisible()
    await expect(page.locator('pre').filter({ hasText: /build:|runner:/ })).toBeVisible()

    await page.getByRole('link', { name: /Артефакты/ }).first().click()
    await expect(page.getByRole('heading', { name: 'Артефакты' })).toBeVisible()
    await expect(page.getByText(expectedArtifactName)).toBeVisible()
    // Downloads use the authenticated API client rather than an unauthenticated link.
    const download = page.waitForEvent('download')
    await page.getByRole('button', { name: 'Скачать' }).first().click()
    const downloadedFile = await download
    expect(downloadedFile.suggestedFilename()).toBe(expectedArtifactName)
  })

  test('[REQ-UI-001] opens repository code and renders the committed Forge pipeline config', async ({ page, request }) => {
    await waitForEvidence(request)

    await page.goto('/repositories/platform-core')
    await expect(page.getByRole('heading', { name: 'platform-core' })).toBeVisible()

    await page.getByRole('tab', { name: 'Код' }).click()
    await page.getByRole('button', { name: /\.forge-ci\.yml/ }).click()

    await expect(page.getByText('.forge-ci.yml')).toBeVisible()
    await expect(page.locator('pre')).toContainText('version: 1')
    await expect(page.locator('pre')).toContainText('compile:')
    await expect(page.locator('pre')).toContainText('target/release/app.tar.gz')
  })

  test('[NFR-UX-01] keeps the mobile drawer keyboard contract', async ({ page, request }) => {
    await waitForEvidence(request)
    await page.setViewportSize({ width: 375, height: 812 })

    await page.goto('/')
    const menuButton = page.getByRole('button', { name: 'Переключить меню' })
    await menuButton.click()

    const dialog = page.getByRole('dialog', { name: 'Переключить меню' })
    await expect(dialog).toBeVisible()
    await expect.poll(() => dialog.evaluate((element) => element.contains(document.activeElement))).toBe(true)
    await page.keyboard.press('Shift+Tab')
    await expect.poll(() => dialog.evaluate((element) => element.contains(document.activeElement))).toBe(true)
    await page.keyboard.press('Escape')
    await expect(dialog).toBeHidden()
    await expect(menuButton).toBeFocused()
  })
})
