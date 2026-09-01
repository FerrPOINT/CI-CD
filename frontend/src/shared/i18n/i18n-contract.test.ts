import { afterEach, describe, expect, it } from 'vitest'
import type { ChangeStatus, PullRequestAction, PullRequestStatus, RunnerStatus, Status } from '@/api/types'
import i18n from './config'
import en from './locales/en.json'
import ru from './locales/ru.json'

interface TranslationTree {
  [key: string]: string | TranslationTree
}

const localeTrees = {
  en: en as TranslationTree,
  ru: ru as TranslationTree,
}

const pipelineStatuses = keysOf({
  queued: true,
  running: true,
  success: true,
  failed: true,
  canceled: true,
} satisfies Record<Status, true>)
const changeStatuses = keysOf({
  added: true,
  modified: true,
  deleted: true,
} satisfies Record<ChangeStatus, true>)
const pullRequestStatuses = keysOf({
  open: true,
  closed: true,
  merged: true,
} satisfies Record<PullRequestStatus, true>)
const pullRequestActions = keysOf({
  merge: true,
  close: true,
  reopen: true,
} satisfies Record<PullRequestAction, true>)
const runnerStatuses = keysOf({
  online: true,
  offline: true,
  paused: true,
} satisfies Record<RunnerStatus, true>)
const deliveryStatuses = ['pending', 'retryScheduled', 'delivered', 'failed']
const notificationStatuses = ['pending', 'delivered', 'failed']
const tokenScopes = ['api:read', 'api:write', 'git:read', 'git:write']

const dynamicTranslationKeys = [
  ...pipelineStatuses.map((status) => `pipelines.${status}`),
  ...changeStatuses.map((status) => `compare.status_${status}`),
  ...pullRequestStatuses.map((status) => `pulls.status_${status}`),
  ...pullRequestActions.map((action) => `pulls.action_${action}`),
  ...runnerStatuses.map((status) => `runners.status_${status}`),
  ...['expired', 'purged'].map((state) => `artifacts.${state}`),
  ...deliveryStatuses.map((status) => `deliveries.${status}`),
  ...notificationStatuses.map((status) => `notifications.${status}`),
  ...tokenScopes.map((scope) => `tokens.scopeLabels.${scope.replace(':', '')}`),
]

afterEach(async () => {
  await i18n.changeLanguage('ru')
})

describe('i18n contract', () => {
  it('keeps ru and en translation keys in parity', () => {
    expect(flatten(localeTrees.ru).keys).toEqual(flatten(localeTrees.en).keys)
  })

  it('does not ship empty values or raw-key fallbacks', () => {
    for (const [locale, tree] of Object.entries(localeTrees)) {
      for (const [key, value] of flatten(tree).entries) {
        expect(value.trim(), `${locale}:${key}`).not.toBe('')
        expect(value, `${locale}:${key}`).not.toBe(key)
      }
    }
  })

  it('covers dynamic UI keys built from stable API contract values', () => {
    for (const key of dynamicTranslationKeys) {
      expect(leaf(localeTrees.ru, key), `ru:${key}`).toEqual(expect.any(String))
      expect(leaf(localeTrees.en, key), `en:${key}`).toEqual(expect.any(String))
    }
  })

  it('switches runtime language without losing configured resources', async () => {
    await i18n.changeLanguage('en')
    expect(i18n.t('navigation.dashboard')).toBe(leaf(localeTrees.en, 'navigation.dashboard'))
    expect(i18n.t('pipelines.success')).toBe(leaf(localeTrees.en, 'pipelines.success'))

    await i18n.changeLanguage('ru')
    expect(i18n.t('navigation.dashboard')).toBe(leaf(localeTrees.ru, 'navigation.dashboard'))
    expect(i18n.t('pipelines.success')).toBe(leaf(localeTrees.ru, 'pipelines.success'))
  })
})

function flatten(tree: TranslationTree) {
  const entries = flattenEntries(tree).sort(([left], [right]) => left.localeCompare(right))
  return {
    entries,
    keys: entries.map(([key]) => key),
  }
}

function flattenEntries(tree: TranslationTree, prefix = ''): Array<[string, string]> {
  return Object.entries(tree).flatMap(([key, value]) => {
    const nextKey = prefix ? `${prefix}.${key}` : key
    return typeof value === 'string' ? [[nextKey, value]] : flattenEntries(value, nextKey)
  })
}

function leaf(tree: TranslationTree, key: string): string | undefined {
  let cursor: string | TranslationTree | undefined = tree
  for (const segment of key.split('.')) {
    if (!cursor || typeof cursor === 'string') {
      return undefined
    }
    cursor = cursor[segment]
  }
  return typeof cursor === 'string' ? cursor : undefined
}

function keysOf<T extends string>(record: Record<T, true>): T[] {
  return Object.keys(record) as T[]
}
