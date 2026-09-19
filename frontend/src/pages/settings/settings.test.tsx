import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { SettingsPage } from './index'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

afterEach(cleanup)

describe('SettingsPage', () => {
  it('filters variables by key and description and can clear the search', () => {
    render(<SettingsPage />)
    const search = screen.getByRole('searchbox', { name: 'settings.search' })

    fireEvent.change(search, { target: { value: 'CICD_RUNNER_TAGS' } })
    expect(screen.getByText('CICD_RUNNER_TAGS')).toBeInTheDocument()
    expect(screen.queryByText('CICD_DATABASE_URL')).not.toBeInTheDocument()

    fireEvent.change(search, { target: { value: 'settings.env.databaseUrl' } })
    expect(screen.getByText('CICD_DATABASE_URL')).toBeInTheDocument()
    expect(screen.queryByText('CICD_RUNNER_TAGS')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'settings.clearSearch' }))
    expect(screen.getByText('CICD_RUNNER_TAGS')).toBeInTheDocument()
  })

  it('shows an empty search result', () => {
    render(<SettingsPage />)
    fireEvent.change(screen.getByRole('searchbox', { name: 'settings.search' }), { target: { value: 'no-such-variable' } })
    expect(screen.getByRole('status')).toHaveTextContent('settings.noMatches')
  })
})
