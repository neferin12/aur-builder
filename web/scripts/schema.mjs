// Introspect only disposable databases populated by the Rust migrations.
import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const root = resolve(fileURLToPath(new URL('..', import.meta.url)))
const check = process.argv.includes('--check')
const temp = mkdtempSync(join(tmpdir(), 'aur-schema-'))
try {
  for (const provider of ['sqlite', 'postgresql']) {
    const key = `PRISMA_${provider.toUpperCase()}_URL`
    if (!process.env[key]) throw new Error(`${key} must point to a disposable, Rust-migrated database`)
    const original = join(root, 'prisma', `${provider}.prisma`)
    const path = join(temp, `${provider}.prisma`)
    writeFileSync(path, `generator client {\n provider = "prisma-client-js"\n engineType = "client"\n output = "../generated/${provider}"\n}\ndatasource db {\n provider = "${provider}"\n url = env("${key}")\n}\n`)
    execFileSync(join(root, 'node_modules/.bin/prisma'), ['db', 'pull', '--schema', path], { stdio: 'inherit' })
    let schema = readFileSync(path, 'utf8')
    // SQLite permits nullable TEXT PRIMARY KEYs. All application writers use UUIDs.
    // Treat PKs as required so Prisma can expose these existing tables.
    schema = schema.replace(/^\/\/\/ The underlying table.*\n/gm, '').replace(/String\?(\s+@id)/g, 'String$1').replace(/^\s*@@ignore\n/gm, '')
    // Existing foreign key uses BIGINT while its target is INTEGER. Preserve scalar
    // types and query explicitly rather than inventing an invalid Prisma relation.
    schema = schema.replace(/^\s*package_metadata\s+package_metadata.*\n/gm, '').replace(/^\s*build_results\s+build_results\[\].*\n/gm, '')
    writeFileSync(path, schema)
    execFileSync(join(root, 'node_modules/.bin/prisma'), ['format', '--schema', path], { stdio: 'inherit' })
    schema = readFileSync(path, 'utf8')
    if (check) {
      if (schema !== readFileSync(original, 'utf8')) throw new Error(`${provider} Prisma schema drift; run npm run db:introspect`)
    } else writeFileSync(original, schema)
  }
} finally { rmSync(temp, { recursive: true, force: true }) }
