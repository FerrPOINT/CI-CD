// @vitest-environment node
import { expect, it } from 'vitest'

it('keeps pure Node tests free of browser globals and storage fixtures', () => {
  expect(typeof window).toBe('undefined')
  expect(typeof document).toBe('undefined')
  expect(typeof localStorage).toBe('undefined')
  expect(typeof sessionStorage).toBe('undefined')
})
