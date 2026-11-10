import { expect, test } from '@playwright/test'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
test('production server exits when the shared database has not been migrated', () => {
  const directory = mkdtempSync(join(tmpdir(), 'aur-unmigrated-'))
  try {
    const child = spawnSync(process.execPath, ['.output/server/index.mjs'], {
      encoding: 'utf8', timeout: 15000,
      env: { ...process.env, PORT: '0', DATABASE_URL: `sqlite:${join(directory, 'empty.db')}`,
        AB_WEB_URL: 'https://builder.example', AB_OIDC_ISSUER: 'https://issuer.example',
        AB_OIDC_CLIENT_ID: 'test', AB_OIDC_CLIENT_SECRET: 'test', AB_OIDC_ADMIN_GROUP: 'admins' },
    })
    expect(child.status).toBe(1)
    expect(child.stderr).toContain('Cannot start Nuxt')
  } finally { rmSync(directory, { recursive: true, force: true }) }
})
