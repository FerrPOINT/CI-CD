import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { mkdtemp, mkdir, readFile, realpath, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import test from 'node:test'

const frontend = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const require = createRequire(import.meta.url)
const plugin = createRequire(require.resolve('@tailwindcss/vite'))
const { compile } = plugin('@tailwindcss/node')
const { Scanner } = plugin('@tailwindcss/oxide')
const stylesheet = await readFile(join(frontend, 'src/index.css'), 'utf8')

async function fixture(run) {
  const temporary = await realpath(tmpdir())
  const directory = await realpath(await mkdtemp(join(temporary, 'forge-css-inputs-')))
  assert.equal(dirname(directory), temporary)
  const root = join(directory, 'CI-CD/frontend')
  const ui = resolve(root, '../../services-base/base-modules/ui/src')
  async function put(name, text) {
    const path = join(root, name)
    await mkdir(dirname(path), { recursive: true })
    await writeFile(path, text)
  }
  try {
    // Isolated token import: this unit checks source selection, not an installed UI pin.
    await writeFile(join(directory, 'tokens.css'), ':root { --fixture-token: 1; }')
    await mkdir(ui, { recursive: true })
    await writeFile(join(ui, 'Button.tsx'), '<button className="min-h-[219px]" />')
    await put('src/Card.tsx', '<div className="h-[117px]" />')
    await put('src/contest.tsx', '<div className="max-w-[227px]" />')
    await put('index.html', '<body class="max-h-[223px]"></body>')
    await put('src/index.css', stylesheet)
    async function render(entry = 'src/index.css') {
      const path = join(root, entry)
      const compiler = await compile(await readFile(path, 'utf8'), {
        base: dirname(path),
        from: path,
        onDependency() {},
        async customCssResolver(id) {
          if (id === 'tailwindcss') return require.resolve('tailwindcss/index.css')
          if (id === '@pdlc/ui/tokens.css') return join(directory, 'tokens.css')
        },
      })
      // Match @tailwindcss/vite's root/source selection with a private tiny tree.
      const sources =
        compiler.root === 'none'
          ? []
          : [compiler.root ?? { base: root, pattern: '**/*', negated: false }]
      const scanner = new Scanner({ sources: [...sources, ...compiler.sources] })
      return { css: compiler.build(scanner.scan()), files: scanner.files }
    }
    await run({ root, ui, put, render })
  } finally {
    assert.equal(await realpath(directory), directory)
    assert.equal(dirname(directory), temporary)
    await rm(directory, { recursive: true, force: true })
  }
}

test('unit, setup, e2e and tooling edits do not change production CSS', async () => {
  await fixture(async ({ put, render }) => {
    const before = await render()
    for (const path of [
      'src/Card.test.tsx',
      'src/nested/Card.spec.ts',
      'src/test/setup.ts',
      'src/test-setup.ts',
      'e2e/flow.ts',
      'vitest.config.ts',
      'tsconfig.test.json',
      'scripts/tooling.mjs',
    ]) {
      await put(path, '<div className="w-[113px]" />')
      const after = await render()
      assert.ok(
        !/width:\s*113px/.test(after.css),
        `Test/tool input reached production CSS: ${path}`,
      )
      assert.equal(after.css, before.css, `Production CSS changed after ${path}`)
    }
  })
})

test('application, index and shared UI fixture classes remain production inputs', async () => {
  await fixture(async ({ put, render }) => {
    const before = await render()
    assert.match(before.css, /height:\s*117px/)
    assert.match(before.css, /min-height:\s*219px/)
    assert.match(before.css, /max-height:\s*223px/)
    assert.match(before.css, /max-width:\s*227px/)
    await put('src/Card.tsx', '<div className="w-[127px]" />')
    const after = await render()
    assert.match(after.css, /width:\s*127px/)
    assert.notEqual(after.css, before.css)
  })
})
