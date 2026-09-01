import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Locator, type Page } from '@playwright/test'
import { evidenceRepositoryName, expectedArtifactName, readEvidencePullRequest, waitForEvidence } from './evidence'

type A11yRoute = {
  name: string
  path: string
  ready: (page: Page) => Locator
  shell?: boolean
}

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
  test('[NFR-UX-01] has no serious or critical axe violations on all baseline routes', async ({ page, request }) => {
    test.setTimeout(150_000)
    const evidence = await waitForEvidence(request)
    const pullRequest = await readEvidencePullRequest(request)

    const routes: A11yRoute[] = [
      { name: 'Dashboard', path: '/', ready: page => page.getByText(evidence.project.name).first(), shell: true },
      { name: 'Projects', path: '/projects', ready: page => page.getByText(evidence.project.name).first(), shell: true },
      {
        name: 'Project pipelines',
        path: `/projects/${evidence.project.id}/pipelines`,
        ready: page => page.getByText(evidence.pipeline.git_ref).first(),
        shell: true,
      },
      { name: 'Pipeline detail', path: `/pipelines/${evidence.pipeline.id}`, ready: page => page.getByText(evidence.job.name).first(), shell: true },
      { name: 'Repositories', path: '/repositories', ready: page => page.getByText(evidenceRepositoryName).first(), shell: true },
      {
        name: 'Repository browser',
        path: `/repositories/${evidenceRepositoryName}`,
        ready: page => page.getByText('ci: add forge pipeline').first(),
        shell: true,
      },
      {
        name: 'Compare',
        path: `/repositories/${evidenceRepositoryName}/compare?from=main&to=feature%2Fcache-layer`,
        ready: page => page.getByText('feature.md').first(),
        shell: true,
      },
      {
        name: 'Pull requests',
        path: `/repositories/${evidenceRepositoryName}/pulls`,
        ready: page => page.getByText(pullRequest.title).first(),
        shell: true,
      },
      {
        name: 'Pull request detail',
        path: `/repositories/${evidenceRepositoryName}/pulls/${pullRequest.number}`,
        ready: page => page.getByText(pullRequest.description).first(),
        shell: true,
      },
      { name: 'Settings', path: '/settings', ready: page => page.getByText('CICD_RUNNER_REGISTRATION_TOKEN').first(), shell: true },
      { name: 'Runners', path: '/runners', ready: page => page.getByRole('row', { name: /docker-runner-01/ }), shell: true },
      { name: 'Project secrets', path: `/projects/${evidence.project.id}/secrets`, ready: page => page.getByText('DEPLOY_TOKEN').first(), shell: true },
      { name: 'Project members', path: `/projects/${evidence.project.id}/members`, ready: page => page.getByRole('row', { name: /a\.zhukov/ }), shell: true },
      { name: 'Artifacts', path: `/jobs/${evidence.job.id}/artifacts`, ready: page => page.getByText(expectedArtifactName).first(), shell: true },
      { name: 'Environments', path: `/projects/${evidence.project.id}/environments`, ready: page => page.getByText('staging').first(), shell: true },
      {
        name: 'Schedules',
        path: `/projects/${evidence.project.id}/schedules`,
        ready: page => page.getByText(/Расписания не настроены\.|0 4 \* \* 1/).first(),
        shell: true,
      },
      { name: 'Webhooks', path: `/projects/${evidence.project.id}/webhooks`, ready: page => page.getByRole('heading', { name: 'История доставок' }), shell: true },
      { name: 'Reports', path: `/projects/${evidence.project.id}/reports`, ready: page => page.getByText('Доля успехов').first(), shell: true },
      { name: 'Audit log', path: '/audit-log', ready: page => page.getByText(/project\.created|События аудита отсутствуют\./).first(), shell: true },
      { name: 'Users', path: '/users', ready: page => page.getByRole('row', { name: /a\.zhukov/ }), shell: true },
      { name: 'Login', path: '/login', ready: page => page.getByRole('heading', { name: 'Вход в Forge' }) },
    ]

    for (const route of routes) {
      await test.step(route.name, async () => {
        await page.goto(route.path)
        if (route.shell) await expect(page.locator('main')).toBeVisible()
        await expect(route.ready(page)).toBeVisible()
        await page.waitForTimeout(250)
        await expectNoSeriousAxeViolations(page)
      })
    }
  })
})
