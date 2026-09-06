// Screenshot: /forbidden (K2 403 UX) — 1920×1080, desktop full-page disabled (fixed viewport).
import { chromium } from '@playwright/test'
import { mkdirSync } from 'node:fs'

const OUT = process.argv[2] ?? '../docs/screenshots/23-forbidden.png'

const browser = await chromium.launch()
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } })
await page.goto('http://127.0.0.1:22802/forbidden')
await page.waitForTimeout(700)
mkdirSync(new URL('.', `file://${process.cwd()}/`), { recursive: true })
await page.screenshot({ path: OUT, fullPage: false })
await browser.close()
console.log('saved', OUT)
