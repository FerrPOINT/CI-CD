import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { UsersPage } from './index'

describe('UsersPage', () => {
  it('points to central user management without local credentials or roles', () => {
    render(<UsersPage />)
    expect(screen.getByRole('link', { name: /Открыть пользователей/ })).toHaveAttribute('href', 'http://localhost:7772/users')
    expect(screen.queryByLabelText(/password|пароль|role|роль/i)).not.toBeInTheDocument()
  })
})
