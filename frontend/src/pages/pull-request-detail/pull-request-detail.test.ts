import { describe, expect, it, vi } from 'vitest'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

import type { PullRequest } from '@/api/types'
import { buildCompareHref } from './index'

const pr: PullRequest = {
  id: 'p1', repository_name: 'platform-core', number: 7, title: 'Add cache', description: '',
  source_branch: 'feature/cache', target_branch: 'main', status: 'open', created_by: 'azhukov',
  created_at: '2026-08-27T00:00:00Z', updated_at: '2026-08-27T00:00:00Z', merged_at: null, merge_commit_sha: null,
}

describe('buildCompareHref', () => {
  it('links to compare with from=target and to=source', () => {
    expect(buildCompareHref(pr)).toBe('/repositories/platform-core/compare?from=main&to=feature%2Fcache')
  })

  it('encodes repository name', () => {
    expect(buildCompareHref({ ...pr, repository_name: 'web app' })).toBe('/repositories/web%20app/compare?from=main&to=feature%2Fcache')
  })
})
