import { chooseValue } from '../../test/choice'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { createMemoryRouter, Link, RouterProvider, useNavigate } from 'react-router'
import { RepositorySyncPreview } from './preview'
import '../../shared/i18n/config'

const routers: ReturnType<typeof createMemoryRouter>[] = []
afterEach(() => {
  cleanup()
  routers.splice(0).forEach((router) => router.dispose())
})

function Back() {
  const navigate = useNavigate()
  return <button onClick={() => navigate(-1)}>Назад в браузере</button>
}
function setup(list = false) {
  const router = createMemoryRouter([
    { path: '/sync-preview.html', element: <><RepositorySyncPreview /><Back /><Link to="/outside">Другой раздел</Link><Link to="?production=1">Другой предпросмотр</Link></> },
    { path: '/outside', element: <h1>Другой раздел открыт</h1> },
  ], { initialEntries: [list ? '/sync-preview.html' : '/sync-preview.html?repository=service-api'] })
  routers.push(router)
  render(<RouterProvider router={router} />)
  return { router }
}
function connect() {
  fireEvent.change(screen.getByLabelText('Токен доступа'), { target: { value: 'demo-token' } })
  fireEvent.click(screen.getByRole('radio', { name: /В обе стороны/ }))
  fireEvent.click(screen.getByRole('button', { name: 'Сохранить подключение' }))
}

describe('repository synchronization preview', () => {
  it('protects a retained draft from reload even when its editor is closed', () => {
    setup()
    fireEvent.change(screen.getByLabelText('Группа / репозиторий'), { target: { value: 'team/retained' } })
    const inEditor = new Event('beforeunload', { cancelable: true })
    window.dispatchEvent(inEditor)
    expect(inEditor.defaultPrevented).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Все синхронизации' }))
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
    const inList = new Event('beforeunload', { cancelable: true })
    window.dispatchEvent(inList)
    expect(inList.defaultPrevented).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'platform / service-api' }))
    expect(screen.getByLabelText('Группа / репозиторий')).toHaveValue('team/retained')
  })

  it('protects another repository draft while the selected connection is clean', () => {
    setup()
    fireEvent.change(screen.getByLabelText('Группа / репозиторий'), { target: { value: 'team/retained' } })
    fireEvent.click(screen.getByRole('button', { name: 'Все синхронизации' }))
    fireEvent.click(screen.getByRole('button', { name: 'platform / web-portal' }))
    expect(screen.getByLabelText('Владелец / репозиторий')).toHaveValue('example-team/web-portal')
    const whileOtherIsClean = new Event('beforeunload', { cancelable: true })
    window.dispatchEvent(whileOtherIsClean)
    expect(whileOtherIsClean.defaultPrevented).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Все синхронизации' }))
    fireEvent.click(screen.getByRole('button', { name: 'platform / service-api' }))
    fireEvent.click(screen.getByRole('button', { name: 'Отменить изменения' }))
    const afterDiscard = new Event('beforeunload', { cancelable: true })
    window.dispatchEvent(afterDiscard)
    expect(afterDiscard.defaultPrevented).toBe(false)
  })

  it('uses the Base leave dialog and preserves the draft and focus when leaving is cancelled', async () => {
    const { router } = setup()
    fireEvent.change(screen.getByLabelText('Группа / репозиторий'), { target: { value: 'team/local-draft' } })
    const opener = screen.getByRole('link', { name: 'Другой раздел' })
    opener.focus()
    fireEvent.click(opener)
    expect(await screen.findByRole('alertdialog', { name: 'Покинуть страницу?' })).toBeVisible()
    fireEvent.click(screen.getByRole('button', { name: 'Остаться' }))
    await waitFor(() => expect(opener).toHaveFocus())
    expect(router.state.location.pathname).toBe('/sync-preview.html')
    expect(screen.getByLabelText('Группа / репозиторий')).toHaveValue('team/local-draft')
    fireEvent.click(opener)
    fireEvent.click(screen.getByRole('button', { name: 'Покинуть' }))
    expect(await screen.findByRole('heading', { name: 'Другой раздел открыт' })).toBeVisible()
    const afterLeaving = new Event('beforeunload', { cancelable: true })
    window.dispatchEvent(afterLeaving)
    expect(afterLeaving.defaultPrevented).toBe(false)
  })

  it('protects switching preview mode but allows internal tabs and saved settings to leave normally', async () => {
    const { router } = setup()
    fireEvent.change(screen.getByLabelText('Группа / репозиторий'), { target: { value: 'team/changed' } })
    fireEvent.click(screen.getByRole('tab', { name: 'История запусков' }))
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('link', { name: 'Другой предпросмотр' }))
    expect(await screen.findByRole('alertdialog')).toBeVisible()
    fireEvent.click(screen.getByRole('button', { name: 'Остаться' }))
    expect(router.state.location.search).not.toContain('production=1')
    fireEvent.click(screen.getByRole('tab', { name: 'Подключение' }))
    fireEvent.click(screen.getByRole('button', { name: 'Сохранить подключение' }))
    const afterSave = new Event('beforeunload', { cancelable: true })
    window.dispatchEvent(afterSave)
    expect(afterSave.defaultPrevented).toBe(false)
    fireEvent.click(screen.getByRole('link', { name: 'Другой раздел' }))
    expect(await screen.findByRole('heading', { name: 'Другой раздел открыт' })).toBeVisible()
  })
  it('keeps a running attempt when navigating back to the list and shows its result', async () => {
    setup()
    fireEvent.click(screen.getByRole('button', { name: 'Синхронизировать сейчас' }))
    fireEvent.click(screen.getByRole('button', { name: 'Все синхронизации' }))
    expect(screen.getByText('Выполняется')).toBeVisible()
    await waitFor(() => expect(screen.getByText('Успешно')).toBeVisible())
    fireEvent.click(screen.getByRole('button', { name: 'История platform / service-api' }))
    expect(screen.getByRole('tab', { name: 'История запусков (1)' })).toHaveAttribute('aria-selected', 'true')
  })

  it('lists both providers and preserves a manual launch setting across navigation', () => {
    setup(true)
    expect(screen.getByRole('heading', { name: 'Синхронизации' })).toBeVisible()
    expect(screen.getByRole('table', { name: 'Список синхронизаций репозиториев' })).toBeVisible()
    fireEvent.click(screen.getByRole('button', { name: 'platform / service-api' }))
    fireEvent.click(screen.getByRole('radio', { name: 'Вручную' }))
    fireEvent.click(screen.getByRole('button', { name: 'Сохранить подключение' }))
    expect(screen.getByRole('button', { name: 'Синхронизировать сейчас' })).toBeEnabled()
    fireEvent.click(screen.getByRole('button', { name: 'Все синхронизации' }))
    expect(screen.getAllByText('Вручную')).toHaveLength(2)
    fireEvent.click(screen.getByRole('button', { name: 'platform / service-api' }))
    expect(screen.getByRole('radio', { name: 'Вручную' })).toBeChecked()
  })

  it('filters the complete list and preserves unsaved drafts when returning to it', async () => {
    setup(true)
    await chooseValue(screen.getByLabelText('Провайдер', { selector: '#provider-filter' }), 'github')
    expect(screen.getByText('Показано 1 из 2')).toBeVisible()
    fireEvent.click(screen.getByRole('button', { name: 'platform / web-portal' }))
    fireEvent.change(screen.getByLabelText('Владелец / репозиторий'), { target: { value: 'team/new-draft' } })
    fireEvent.click(screen.getByRole('button', { name: 'Все синхронизации' }))
    fireEvent.click(screen.getByRole('button', { name: 'platform / web-portal' }))
    expect(screen.getByLabelText('Владелец / репозиторий')).toHaveValue('team/new-draft')
    expect(screen.getByText('Есть несохранённые изменения')).toBeVisible()
  })

  it('adds a connection to the list only after it has been saved', () => {
    setup(true)
    fireEvent.click(screen.getByRole('button', { name: 'Добавить синхронизацию' }))
    fireEvent.click(screen.getByRole('button', { name: 'Настроить подключение' }))
    fireEvent.change(screen.getByLabelText('Токен доступа'), { target: { value: 'demo-worker-token' } })
    fireEvent.click(screen.getByRole('radio', { name: /Внешний → Forge/ }))
    fireEvent.click(screen.getByRole('radio', { name: 'Вручную' }))
    fireEvent.click(screen.getByRole('button', { name: 'Сохранить подключение' }))
    fireEvent.click(screen.getByRole('button', { name: 'Все синхронизации' }))
    expect(screen.getByText('Показано 3 из 3')).toBeVisible()
    expect(screen.getByRole('button', { name: 'platform / worker' })).toBeVisible()
  })

  it('preserves an unsaved draft and returns to connection on browser Back', async () => {
    setup()
    fireEvent.change(screen.getByLabelText('Группа / репозиторий'), { target: { value: 'team/changed' } })
    fireEvent.click(screen.getByRole('tab', { name: 'История запусков' }))
    fireEvent.click(screen.getByRole('button', { name: 'Назад в браузере' }))
    await waitFor(() => expect(screen.getByRole('tab', { name: 'Подключение' })).toHaveAttribute('aria-selected', 'true'))
    expect(screen.getByLabelText('Группа / репозиторий')).toHaveValue('team/changed')
  })

  it('keeps history and credentials when synchronization is disabled', async () => {
    setup(); connect()
    fireEvent.click(screen.getByRole('button', { name: 'Синхронизировать сейчас' }))
    await waitFor(() => expect(screen.getByRole('tab', { name: 'История запусков (1)' })).toBeVisible())
    fireEvent.click(screen.getByRole('radio', { name: /Отключено/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Сохранить подключение' }))
    expect(screen.getByRole('button', { name: 'Синхронизировать сейчас' })).toBeDisabled()
    expect(screen.getByText('Синхронизация отключена')).toBeVisible()
    expect(screen.getByRole('tab', { name: 'История запусков (1)' })).toBeVisible()
    expect(screen.getByLabelText('Токен доступа')).toHaveValue('')
  })

  it('requires new credentials for a changed provider and restores the saved connection on cancel', async () => {
    setup(); connect()
    await chooseValue(screen.getByLabelText('Провайдер'), 'github')
    fireEvent.click(screen.getByRole('button', { name: 'Сохранить подключение' }))
    expect(screen.getByRole('alert')).toHaveTextContent('Введите демонстрационный токен доступа.')
    fireEvent.click(screen.getByRole('button', { name: 'Отменить изменения' }))
    fireEvent.click(screen.getByRole('button', { name: 'Проверить доступ' }))
    expect(screen.getByRole('status')).toHaveTextContent('чтение и запись доступны')
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('clears an old success on an access failure and disables mutations for read-only access', async () => {
    setup(); connect()
    fireEvent.click(screen.getByRole('button', { name: 'Проверить доступ' }))
    fireEvent.click(screen.getByText('Сценарии предпросмотра'))
    await chooseValue(screen.getByLabelText('Результат следующего запуска'), 'auth')
    fireEvent.click(screen.getByRole('button', { name: 'Проверить доступ' }))
    expect(screen.getByRole('alert')).toHaveTextContent('Доступ отклонён')
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
    fireEvent.click(screen.getByLabelText('Только чтение'))
    expect(screen.getByLabelText('Провайдер')).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Сохранить подключение' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Синхронизировать сейчас' })).toBeDisabled()
  })
})
