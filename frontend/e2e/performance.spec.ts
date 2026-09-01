import { expect, test, type APIRequestContext, type Locator, type Page } from '@playwright/test'
import { waitForEvidence } from './evidence'

const apiBaseURL = process.env.E2E_API_URL ?? 'http://127.0.0.1:22801/api/v1'
const apiReadP95BudgetMs = Number(process.env.E2E_API_READ_P95_BUDGET_MS ?? 1000)
const dashboardReadyBudgetMs = Number(process.env.E2E_DASHBOARD_READY_BUDGET_MS ?? 5000)

type TimingSample = {
  name: string
  durationMs: number
}

test.describe('performance smoke', () => {
  test('[NFR-PERF-01] keeps seeded Dashboard and API reads within MVP regression budgets', async ({ page, request }) => {
    const evidence = await waitForEvidence(request)
    const readPaths = [
      '/projects',
      `/projects/${evidence.project.id}/pipelines`,
      `/pipelines/${evidence.pipeline.id}`,
      `/jobs/${evidence.job.id}/artifacts`,
    ]

    for (const path of readPaths) {
      await timedApiGet(request, path)
    }

    const apiSamples: TimingSample[] = []
    for (let round = 0; round < 5; round += 1) {
      for (const path of readPaths) {
        apiSamples.push(await timedApiGet(request, path))
      }
    }

    const apiP95 = percentile(apiSamples.map(sample => sample.durationMs), 95)
    expect(apiP95, `API read p95 ${apiP95.toFixed(1)} ms; slowest ${formatSlowest(apiSamples)}`)
      .toBeLessThanOrEqual(apiReadP95BudgetMs)

    const dashboardSamples = [
      await timedPageReady(page, '/', page.getByRole('heading', { name: 'Дашборд' })),
      await timedPageReady(page, `/pipelines/${evidence.pipeline.id}`, page.getByText('План запуска')),
      await timedPageReady(page, `/jobs/${evidence.job.id}/artifacts`, page.getByRole('heading', { name: 'Артефакты' })),
    ]
    const slowestDashboard = Math.max(...dashboardSamples.map(sample => sample.durationMs))

    expect(
      slowestDashboard,
      `Dashboard route max ${slowestDashboard.toFixed(1)} ms; samples ${formatSlowest(dashboardSamples, dashboardSamples.length)}`,
    ).toBeLessThanOrEqual(dashboardReadyBudgetMs)
  })
})

async function timedApiGet(request: APIRequestContext, path: string): Promise<TimingSample> {
  const startedAt = performance.now()
  const response = await request.get(`${apiBaseURL}${path}`)
  const durationMs = performance.now() - startedAt
  if (!response.ok()) {
    const body = await response.text()
    expect(response.ok(), `${path} -> ${response.status()} ${body.slice(0, 300)}`).toBeTruthy()
  }
  return { name: path, durationMs }
}

async function timedPageReady(page: Page, path: string, readyLocator: Locator): Promise<TimingSample> {
  const startedAt = performance.now()
  await page.goto(path, { waitUntil: 'domcontentloaded' })
  await expect(readyLocator).toBeVisible()
  return { name: path, durationMs: performance.now() - startedAt }
}

function percentile(values: number[], percentileValue: number): number {
  if (values.length === 0) return 0
  const sorted = [...values].sort((left, right) => left - right)
  const index = Math.min(sorted.length - 1, Math.ceil((percentileValue / 100) * sorted.length) - 1)
  return sorted[index]
}

function formatSlowest(samples: TimingSample[], count = 3): string {
  return [...samples]
    .sort((left, right) => right.durationMs - left.durationMs)
    .slice(0, count)
    .map(sample => `${sample.name}=${sample.durationMs.toFixed(1)}ms`)
    .join(', ')
}
