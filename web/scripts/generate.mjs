import { execFileSync } from 'node:child_process'
import { rmSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
const root = fileURLToPath(new URL('..', import.meta.url))
// Remove old native-engine artifacts when switching engine modes or providers.
rmSync(new URL('../generated', import.meta.url), { recursive: true, force: true })
for (const provider of ['sqlite', 'postgresql']) execFileSync(`${root}/node_modules/.bin/prisma`, ['generate', '--schema', `prisma/${provider}.prisma`], { cwd: root, stdio: 'inherit' })
