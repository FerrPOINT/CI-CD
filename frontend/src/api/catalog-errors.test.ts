// @vitest-environment node

import { expect, it } from 'vitest'
import { ApiError } from './client'
import { catalogErrorMessage } from './catalog-errors'

it.each([
  [new ApiError({ kind: 'api', status: 409, message: 'namespace_resource_read_only' }), 'только для чтения'],
  [new ApiError({ kind: 'api', status: 403, message: 'denied' }), 'Нет прав'],
  [new ApiError({ kind: 'network', message: 'Failed to fetch' }), 'Проверьте результат операции'],
  [new Error('Owner diagnostic'), 'Owner diagnostic'],
])('preserves the operation outcome while presenting a readable error', (error, text) => {
  expect(catalogErrorMessage(error)).toContain(text)
})
