import { expect, test } from '@playwright/test'

const prefix = process.env.PLAYWRIGHT_APP_PREFIX

test('serves the prefixed login and keeps its SSO callback in the same product', async ({
  page,
}, testInfo) => {
  test.skip(!prefix, 'Manual gate: build with VITE_APP_BASE_PATH and set PLAYWRIGHT_APP_PREFIX')
  const basePath = prefix!
  const errors: string[] = []
  const assets: string[] = []
  page.on('pageerror', (error) => errors.push(error.message))
  page.on('response', (response) => {
    const path = new URL(response.url()).pathname
    if (/\.(?:js|css)$/.test(path) && response.ok()) assets.push(path)
  })
  const entry = new URL(`${basePath}login?logged_out=1`, testInfo.project.use.baseURL as string)
  await page.goto(entry.href)
  const signIn = page.getByRole('button', { name: /Войти через SSO|Sign in with SSO/ })
  await expect(signIn).toBeEnabled()
  expect(new URL(page.url()).pathname).toBe(`${basePath}login`)
  expect(assets.length).toBeGreaterThan(0)
  expect(assets.every((asset) => asset.startsWith(`${basePath}assets/`))).toBe(true)
  expect(errors).toEqual([])
  for (const width of [375, 1920, 2560]) {
    await page.setViewportSize({ width, height: width === 375 ? 812 : 1080 })
    await expect(signIn).toBeVisible()
    await page.screenshot({
      path: testInfo.outputPath(`prefix-login-${width}.png`),
      fullPage: true,
    })
  }
  const relay = await page.evaluate(async (path) => {
    const iframe = document.createElement('iframe')
    iframe.hidden = true
    iframe.src = path
    return new Promise<{ type: string; state: string; error: string; search: string }>(
      (resolve, reject) => {
        const timer = window.setTimeout(() => {
          window.removeEventListener('message', receive)
          iframe.remove()
          reject(new Error('Prefixed callback did not relay its result'))
        }, 5000)
        function receive(event: MessageEvent) {
          if (event.origin !== window.location.origin || event.source !== iframe.contentWindow)
            return
          window.clearTimeout(timer)
          window.removeEventListener('message', receive)
          const search = iframe.contentWindow.location.search
          iframe.remove()
          resolve({ ...event.data, search })
        }
        window.addEventListener('message', receive)
        document.body.append(iframe)
      },
    )
  }, `${basePath}sso/callback?state=prefix-relay-fixture&error=login_required`)
  expect(relay.type).toMatch(/^(sdlc|pdlc)\.oidc\.response$/)
  expect(relay.state).toBe('prefix-relay-fixture')
  expect(relay.error).toBe('login_required')
  expect(relay.search).toBe('')
  await page.route('**/oidc/authorize**', (route) =>
    route.fulfill({
      contentType: 'text/html',
      body: '<h1>Authorization endpoint fixture</h1>',
    }),
  )
  const authorization = page.waitForRequest(
    (request) =>
      new URL(request.url()).pathname.endsWith('/oidc/authorize') &&
      request.frame() === page.mainFrame(),
  )
  await signIn.click()
  const request = new URL((await authorization).url())
  const callback = new URL(request.searchParams.get('redirect_uri')!)
  expect(callback.origin).toBe(entry.origin)
  expect(callback.pathname).toBe(`${basePath}sso/callback`)
  expect(request.searchParams.get('code_challenge_method')).toBe('S256')
  expect(request.searchParams.get('code_challenge')).toBeTruthy()
  expect(request.searchParams.get('state')).toBeTruthy()
})
