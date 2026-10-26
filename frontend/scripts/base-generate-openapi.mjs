/** One closed-source postinstall producer; broader manual generators stay product-owned. */
import { createHash, randomUUID } from 'node:crypto'
import { createRequire } from 'node:module'
import { existsSync, lstatSync, mkdirSync, readFileSync, realpathSync, renameSync, unlinkSync, writeFileSync } from 'node:fs'
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

function within(root, path) {
  const name = relative(root, path)
  return name !== '..' && !name.startsWith('..' + sep) && !isAbsolute(name)
}

function withoutLinks(path) {
  for (let current = path; ; current = dirname(current)) {
    try {
      if (lstatSync(current).isSymbolicLink()) throw new Error('Generator source/output cannot contain links or junctions')
    } catch (error) {
      if (error.code !== 'ENOENT') throw error
    }
    if (dirname(current) === current) break
  }
}

export function assertClosedSchema(schema) {
  if (!schema || typeof schema !== 'object' || Array.isArray(schema)) throw new Error('Expected a closed OpenAPI schema object')
  const pending = [schema], seen = new WeakSet()
  while (pending.length) {
    const value = pending.pop()
    if (!value || typeof value !== 'object' || seen.has(value)) continue
    seen.add(value)
    for (const [key, item] of Object.entries(value)) {
      if (['$ref', '$dynamicRef', '$id'].includes(key) &&
          (typeof item !== 'string' || item !== '' && !item.startsWith('#'))) {
        throw new Error('Prepared generation requires closed fragment references; qualify other consumers explicitly')
      }
      if (key === 'externalValue') throw new Error('Prepared generation requires closed example inputs')
      if (key === 'discriminator' && item?.mapping && Object.values(item.mapping).some(ref => typeof ref !== 'string' || !ref.startsWith('#'))) {
        throw new Error('Prepared generation requires closed discriminator references')
      }
      if (item && typeof item === 'object') pending.push(item)
    }
  }
}

function compiler(frontend, name, version) {
  const consumer = createRequire(join(frontend, 'package.json'))
  const entry = consumer.resolve(name)
  if (!within(join(frontend, 'node_modules'), entry)) throw new Error('Generator compiler must come from the selected frontend dependencies')
  let directory = dirname(entry), qualified = false
  for (let depth = 0; depth < 4; depth++, directory = dirname(directory)) {
    const manifest = join(directory, 'package.json')
    if (!existsSync(manifest)) continue
    const value = JSON.parse(readFileSync(manifest, 'utf8'))
    if (value.name !== name) continue
    if (value.version !== version) throw new Error('Generator compiler version requires qualification')
    qualified = true
    break
  }
  if (!qualified) throw new Error('Generator compiler ownership is unavailable')
  return consumer(name)
}

export function parseArguments(args) {
  const names = { '--schema': 'schema', '--output': 'output', '--spec-env': 'specVariable' }, result = {}
  for (let index = 0; index < args.length; index += 2) {
    const key = names[args[index]], value = args[index + 1]
    if (!key || !value || value.startsWith('--') || key in result) throw new Error('Invalid or duplicate generator argument')
    result[key] = value
  }
  if (!result.schema || !result.output) throw new Error('Missing generator argument')
  return result
}

export async function generate({ frontend = process.cwd(), schema, output, specVariable, environment = process.env }) {
  frontend = resolve(frontend)
  withoutLinks(frontend)
  if (specVariable && !/^[A-Z_][A-Z0-9_]*$/.test(specVariable)) throw new Error('Invalid generator environment selector')
  const selected = specVariable && environment[specVariable] !== undefined ? environment[specVariable] : schema
  if (typeof selected !== 'string' || selected.includes('\0') || typeof output !== 'string') throw new Error('Invalid generator file argument')
  const sourceRoot = resolve(frontend, '../openapi'), source = resolve(frontend, selected), target = resolve(frontend, output)
  if (!within(sourceRoot, source) || source === sourceRoot) throw new Error('Schema must be inside the product OpenAPI directory')
  if (!within(frontend, target) || !target.endsWith('.ts')) throw new Error('Generated TypeScript must stay inside the frontend')
  withoutLinks(source); withoutLinks(target)
  for (const name of ['redocly.yaml', 'redocly.yml', '.redocly.yaml', '.redocly.yml']) {
    const path = join(frontend, name)
    withoutLinks(path)
    if (existsSync(path)) throw new Error('Generator configuration requires separate qualification')
  }
  if (!lstatSync(source).isFile()) throw new Error('Schema must be a regular source file')
  const bytes = readFileSync(source), text = bytes.toString('utf8').replace(/^\uFEFF/, '')
  const yaml = /\.ya?ml$/i.test(source) ? compiler(frontend, 'yaml', '2.9.0') : undefined
  let document
  try {
    document = yaml ? yaml.parse(text) : JSON.parse(text)
  } catch {
    throw new Error('Source schema is not valid JSON/YAML')
  }
  assertClosedSchema(document)
  const api = compiler(frontend, 'openapi-typescript', '7.13.0')
  const declarations = api.COMMENT_HEADER + api.astToString(await api.default(document))
  mkdirSync(dirname(target), { recursive: true }); withoutLinks(dirname(target))
  const temporary = join(dirname(target), '.' + basename(target) + '.' + randomUUID() + '.tmp')
  try {
    writeFileSync(temporary, declarations, { flag: 'wx' })
    withoutLinks(target)
    renameSync(temporary, target)
  } finally {
    if (existsSync(temporary)) unlinkSync(temporary)
  }
  return { schema: 'base-openapi-generation/v1', input: relative(resolve(frontend, '..'), source).split(sep).join('/'),
    output: relative(frontend, target).split(sep).join('/'), source_sha256: createHash('sha256').update(bytes).digest('hex') }
}

if (process.argv[1] && realpathSync(resolve(process.argv[1])) === fileURLToPath(import.meta.url)) {
  try { console.log(JSON.stringify(await generate(parseArguments(process.argv.slice(2))))) }
  catch (error) { console.error(error.message); process.exitCode = 1 }
}
