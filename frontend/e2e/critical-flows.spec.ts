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
    await expect(page.getByText('target/release/app.tar.gz')).toBeVisible()

    await page.getByRole('button', { name: /Логи/ }).first().click()
    await expect(page.getByText(new RegExp(`Логи.*${evidence.job.name}`))).toBeVisible()
    await expect(page.locator('pre').filter({ hasText: /build:|runner:/ })).toBeVisible()

    await page.getByRole('link', { name: /Артефакты/ }).first().click()
    await expect(page.getByRole('heading', { name: 'Артефакты' })).toBeVisible()
    await expect(page.getByText(expectedArtifactName)).toBeVisible()
    await expect(page.getByRole('link', { name: 'Скачать' }).first()).toHaveAttribute('href', /\/api\/v1\/artifacts\/.+\/download/)
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

    await expect(page.getByRole('dialog', { name: 'Переключить меню' })).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.getByRole('dialog', { name: 'Переключить меню' })).toBeHidden()
    await expect(menuButton).toBeFocused()
  })
})
