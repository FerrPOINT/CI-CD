import { spawnSync } from 'node:child_process'
import { resolve } from 'node:path'

const result = spawnSync(process.execPath, [resolve('../../services-base/frontend/scripts/sdlc-openapi-compat.mjs'), '--base-ref', 'origin/main'], {
  cwd: process.cwd(), stdio: 'inherit', env: { ...process.env, SPEC_NAME: 'openapi/openapi.yaml' },
})
process.exit(result.status ?? 1)
