import { fireEvent, screen, waitFor } from '@testing-library/react'
export async function choiceOptions(field: HTMLElement) {
  fireEvent.keyDown(field, {key: 'Enter'})
  const popup = await screen.findByRole('listbox')
  const items = screen.queryAllByRole('option')
  const options = items.map(item => ({value: item.getAttribute('data-value')!, textContent: item.textContent}))
  fireEvent.keyDown(popup, {key: 'Escape'})
  await waitFor(() => { if (screen.queryByRole('listbox')) throw Error('Choice did not close') })
  return options
}
export async function chooseValue(field: HTMLElement, value: string | string[]) {
  fireEvent.keyDown(field, {key: 'Enter'})
  const multiple = Array.isArray(value)
  const items = await screen.findAllByRole(multiple ? 'menuitemcheckbox' : 'option')
  if (multiple) {
    for (const item of items) if ((item.getAttribute('aria-checked') === 'true') !== value.includes(item.getAttribute('data-value')!)) fireEvent.click(item)
    fireEvent.keyDown(items[0]!, {key: 'Escape'})
  } else {
    const item = items.find(item => item.getAttribute('data-value') === value)
    if (!item) throw Error(`Choice option not found: ${value}`)
    fireEvent.keyDown(item, {key: 'Enter'})
  }
  await waitFor(() => { if (screen.queryByRole(multiple ? 'menu' : 'listbox')) throw Error('Choice did not close') })
  await waitFor(() => { if (field.isConnected && !field.matches(':disabled') && document.activeElement !== field) throw Error('Choice focus has not returned') })
}

export async function findChoiceOption(field: HTMLElement, name: string | RegExp) {
  return waitFor(async () => {
    const option = (await choiceOptions(field)).find(option => typeof name === 'string' ? option.textContent === name : name.test(option.textContent ?? ''))
    if (!option) throw Error(`Choice option not found: ${name}`)
    return option
  })
}
