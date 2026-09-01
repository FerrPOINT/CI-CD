// Deterministic evidence seed for Forge CI/CD screenshots.
// Creates: repositories with real commits, projects, pipelines in mixed states,
// runners, secrets, environments, deployments, audit trail, users, tokens.
import { execFileSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const API = 'http://127.0.0.1:22801/api/v1'
const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
const demoRunnerNames = new Set(['docker-runner-01', 'shell-runner-02'])

async function api(method, path, body) {
  const res = await fetch(`${API}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  })
  const text = await res.text()
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}: ${text.slice(0, 300)}`)
  return text ? JSON.parse(text) : null
}

function git(cwd, ...args) {
  return execFileSync('git', args, { cwd, encoding: 'utf8' })
}

function resetEvidenceAudit() {
  execFileSync('docker', [
    'compose',
    'exec',
    '-T',
    'postgres',
    'sh',
    '-lc',
    'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -v ON_ERROR_STOP=1 -c "TRUNCATE TABLE audit_log RESTART IDENTITY;"',
  ], { cwd: ROOT })
}

async function deleteDemoRunners(runners) {
  for (const runner of runners) {
    await api('DELETE', `/runners/${runner.id}`)
  }
}

async function seedRepository(name, commits, branches = {}) {
  const existing = (await api('GET', '/repositories')).find(r => r.name === name)
  if (!existing) await api('POST', '/repositories', { name })
  const dir = mkdtempSync(join(tmpdir(), `forge-${name}-`))
  git(dir, 'init', '-b', 'main')
  git(dir, 'config', 'user.email', 'azhukov@forge.local')
  git(dir, 'config', 'user.name', 'Александр Жуков')
  const repoUrl = 'http://127.0.0.1:22801/git/' + name + '.git'
  git(dir, 'remote', 'add', 'origin', repoUrl)
  writeFileSync(join(dir, 'README.md'), `# ${name}\n\nForge CI/CD demo repository.\n`)
  git(dir, 'add', '.')
  git(dir, 'commit', '-m', 'chore: initial project skeleton')
  for (const c of commits) {
    const fp = join(dir, c.file)
    mkdirSync(dirname(fp), { recursive: true })
    writeFileSync(fp, c.content)
    git(dir, 'add', '.')
    git(dir, 'commit', '-m', c.message)
  }
  for (const [branch, from] of Object.entries(branches)) {
    git(dir, 'checkout', '-b', branch, from)
    writeFileSync(join(dir, 'feature.md'), `# ${branch}\n`)
    git(dir, 'add', '.')
    git(dir, 'commit', '-m', `feat: ${branch} work`)
    git(dir, 'push', '-f', 'origin', branch)
    git(dir, 'checkout', 'main')
  }
  git(dir, 'push', '-f', 'origin', 'main')
  rmSync(dir, { recursive: true, force: true })
  return name
}

const FORGE_CI = `version: 1
defaults:
  image: alpine:3.21
jobs:
  compile:
    commands:
      - echo "build: compiling platform-core"
      - mkdir -p target/release
      - printf "forge evidence artifact\\n" > target/release/app.tar.gz
      - echo "build: artifact target/release/app.tar.gz ready"
    artifacts:
      paths:
        - target/release/app.tar.gz
  unit:
    needs: [compile]
    commands:
      - echo "test: 42 passed, 0 failed"
  publish:
    needs: [unit]
    commands:
      - echo "deploy: publishing artifacts to registry"
      - echo "deploy: done"
`

async function main() {
  // Clean previous demo data by deleting demo projects (cascades pipelines)
  const projects = await api('GET', '/projects')
  const demoProjects = projects.filter(p => p.name.startsWith('forge-demo-'))
  const existingRunners = await api('GET', '/runners')
  const demoRunners = existingRunners.filter(r => demoRunnerNames.has(r.name))

  for (const p of demoProjects) {
    await api('DELETE', `/projects/${p.id}`)
  }
  await deleteDemoRunners(demoRunners)
  resetEvidenceAudit()

  // Repositories with real content
  await seedRepository('platform-core', [
    { file: '.forge-ci.yml', content: FORGE_CI, message: 'ci: add forge pipeline' },
    { file: 'src/main.rs', content: 'fn main() { println!("platform-core"); }\n', message: 'feat: core entrypoint' },
  ], { 'feature/cache-layer': 'main' })
  await seedRepository('web-frontend', [
    { file: '.forge-ci.yml', content: FORGE_CI, message: 'ci: frontend pipeline' },
    { file: 'index.html', content: '<!doctype html><title>web</title>\n', message: 'feat: landing page' },
  ])
  await seedRepository('api-gateway', [
    { file: '.forge-ci.yml', content: FORGE_CI, message: 'ci: gateway pipeline' },
  ], { 'feature/rate-limit': 'main' })

  // Projects bound to local repos
  const mk = async (name, repo) => {
    const list = await api('GET', '/projects')
    const existing = list.find(p => p.name === name)
    if (existing) return existing
    return api('POST', '/projects', { name, repository_url: `http://backend:22801/git/${repo}.git`, default_branch: 'main' })
  }
  const core = await mk('forge-demo-platform', 'platform-core')
  const web = await mk('forge-demo-web', 'web-frontend')
  const gw = await mk('forge-demo-gateway', 'api-gateway')

  // Pipelines in mixed states: pushed triggers already ran; trigger manual ones too
  for (const [project, ref] of [[core.id, 'main'], [core.id, 'main'], [web.id, 'main'], [gw.id, 'feature/rate-limit'], [gw.id, 'main']]) {
    await api('POST', `/projects/${project}/pipelines`, { git_ref: ref })
  }

  // Users (idempotent, trusted-network mode)
  for (const u of [
    { username: 'a.zhukov', role: 'admin' },
    { username: 'm.petrova', role: 'maintainer' },
    { username: 'd.orlov', role: 'developer' },
  ]) {
    const users = await api('GET', '/users')
    if (!users.some(x => x.username === u.username)) await api('POST', '/users', u)
  }

  const seedUsers = await api('GET', '/users')
  const usersByName = Object.fromEntries(seedUsers.map(user => [user.username, user]))
  for (const project of [core, web, gw]) {
    for (const [username, role] of [
      ['a.zhukov', 'maintainer'],
      ['m.petrova', 'maintainer'],
      ['d.orlov', 'developer'],
    ]) {
      const user = usersByName[username]
      if (user) await api('POST', `/projects/${project.id}/memberships`, { user_id: user.id, role })
    }
  }

  // Pull request on platform-core (idempotent)
  const pulls = await api('GET', '/repos/platform-core/pulls')
  if (!pulls.some(pr => pr.title === 'Add cache layer for registry lookups')) await api('POST', '/repos/platform-core/pulls', {
    repository_name: 'platform-core',
    title: 'Add cache layer for registry lookups',
    description: 'Кеширует резолвы образов, снижает время холодного старта job на ~40%.',
    source_branch: 'feature/cache-layer',
    target_branch: 'main',
    author: 'a.zhukov',
  })

  // Runners
  const runners = await api('GET', '/runners')
  if (!runners.some(r => r.name === 'docker-runner-01')) await api('POST', '/runners', { name: 'docker-runner-01', tags: ['linux', 'docker'] })
  if (!runners.some(r => r.name === 'shell-runner-02')) await api('POST', '/runners', { name: 'shell-runner-02', tags: ['linux', 'shell'] })

  // Secrets
  const secrets = await api('GET', `/projects/${core.id}/secrets`)
  if (!secrets.some(s => s.key === 'DEPLOY_TOKEN')) await api('POST', `/projects/${core.id}/secrets`, { key: 'DEPLOY_TOKEN', value: 'forge-seed-deploy-token-9f2a' })
  if (!secrets.some(s => s.key === 'REGISTRY_PASSWORD')) await api('POST', `/projects/${core.id}/secrets`, { key: 'REGISTRY_PASSWORD', value: 'forge-seed-registry-pw-7c1e' })

  // Environments + deployments
  let envs = await api('GET', `/projects/${core.id}/environments`)
  let staging = envs.find(e => e.name === 'staging')
  if (!staging) staging = await api('POST', `/projects/${core.id}/environments`, { name: 'staging', url: 'https://staging.forge.local' })
  let prod = envs.find(e => e.name === 'production')
  if (!prod) prod = await api('POST', `/projects/${core.id}/environments`, { name: 'production', url: 'https://forge.local' })
  const deps = await api('GET', `/environments/${staging.id}/deployments`)
  if (deps.length === 0) await api('POST', `/environments/${staging.id}/deployments`, { git_ref: 'main' })

  // Users + token
  const users = await api('GET', '/users')
  if (!users.some(u => u.username === 'developer01')) await api('POST', '/users', { username: 'developer01', role: 'developer' })

  console.log(JSON.stringify({ ok: true, projects: 3 + projects.filter(p => !p.name.startsWith('forge-demo-')).length, runners: 2, secrets: 2, envs: [staging.name, prod.name] }))
}

main().catch(err => { console.error(err); process.exit(1) })
