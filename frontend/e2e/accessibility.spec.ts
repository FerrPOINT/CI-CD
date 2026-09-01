import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Page } from '@playwright/test'
import { waitForEvidence } from './evidence'

async function expectNoSeriousAxeViolations(page: Page): Promise<void> {
  const results = await new AxeBuilder({ page }).analyze()
  const violations = results.violations.filter(violation =>
    violation.impact === 'serious' || violation.impact === 'critical'
  )

  expect(
    violations.map(violation => ({
      id: violation.id,
      impact: violation.impact,
      nodes: violation.nodes.map(node => node.target.join(' ')).slice(0, 5),
    })),
  ).toEqual([])
}

test.describe('accessibility smoke', () => {
  test('[NFR-UX-01] has no serious or critical axe violations on representative pages', async ({ page, request }) => {
    const evidence = await waitForEvidence(request)

    for (const path of [
      '/',
      '/projects',
      '/repositories/platform-core',
      `/pipelines/${evidence.pipeline.id}`,
      `/jobs/${evidence.job.id}/artifacts`,
    ]) {
      await page.goto(path)
      await expect(page.locator('main')).toBeVisible()
      await expectNoSeriousAxeViolations(page)
    }
  })
})
