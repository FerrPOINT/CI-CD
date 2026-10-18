import { render, screen } from '@testing-library/react'
import { expect, it } from 'vitest'

it('retains browser storage, matchers and cleanup for DOM tests', () => {
  expect(window.localStorage).toBe(localStorage)
  expect(window.sessionStorage).toBe(sessionStorage)
  localStorage.setItem('setup-neighbor', 'present')
  expect(localStorage.getItem('setup-neighbor')).toBe('present')
  localStorage.removeItem('setup-neighbor')
  render(<button>DOM setup neighbor</button>)
  expect(screen.getByRole('button', { name: 'DOM setup neighbor' })).toBeInTheDocument()
})

it('cleans up the previous test render', () => {
  expect(screen.queryByRole('button', { name: 'DOM setup neighbor' })).toBeNull()
})
