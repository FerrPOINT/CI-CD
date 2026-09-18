import '@testing-library/jest-dom/vitest'

import { cleanup } from '@testing-library/react'
import { afterEach } from 'vitest'
afterEach(() => cleanup())

class MemoryStorage implements Storage {
  private readonly items = new Map<string, string>()

  get length() {
    return this.items.size
  }

  clear() {
    this.items.clear()
  }

  getItem(key: string) {
    return this.items.get(key) ?? null
  }

  key(index: number) {
    return Array.from(this.items.keys())[index] ?? null
  }

  removeItem(key: string) {
    this.items.delete(key)
  }

  setItem(key: string, value: string) {
    this.items.set(key, String(value))
  }
}

function installStorage(name: 'localStorage' | 'sessionStorage') {
  const current = globalThis[name]
  if (current && typeof current.getItem === 'function' && typeof current.setItem === 'function') {
    return
  }

  const storage = new MemoryStorage()
  Object.defineProperty(globalThis, name, { value: storage, configurable: true })
  if (typeof window !== 'undefined') {
    Object.defineProperty(window, name, { value: storage, configurable: true })
  }
}

installStorage('localStorage')
installStorage('sessionStorage')
