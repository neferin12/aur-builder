import { beforeAll, afterAll, describe, expect, it } from 'vitest'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import type { PrismaClient as SQLiteClient } from '../generated/sqlite/index.js'
import { assertSchema, createDatabase, createSQLiteClient, createPostgresClient } from '../server/utils/db'
import { buildLog, decide, history, packages, rebuild, review, reviews, updateAccess } from '../server/utils/repository'
import { hash, now } from '../server/utils/config'
import { identity } from '../server/utils/auth'
import { router } from '../server/rpc/router'
import { createRouterClient } from '@orpc/server'
import { createEvent } from 'h3'
import { IncomingMessage, ServerResponse } from 'node:http'
import { Socket } from 'node:net'
const temp = mkdtempSync(join(tmpdir(), 'aur-web-tests-'))
const payload = { name: 'example', version: '2.0-1', maintainer: 'Alice', last_modified: 1234, source: 'https://user:secret@example.com/source?token=hidden', subfolder: null, options: null, environment: [{ name: 'SECRET', value: 'never-expose-this' }] }
const snapshot = { complete: false, files: [{ path: 'PKGBUILD', content: 'pkgver=2\n<script>bad()</script>', hash: 'new', mode: 33188, size: 40 }] }
Object.assign(process.env, { AB_WEB_URL: 'https://builder.example', AB_OIDC_ISSUER: 'https://issuer.example', AB_OIDC_CLIENT_ID: 'test', AB_OIDC_CLIENT_SECRET: 'secret', AB_OIDC_ADMIN_GROUP: 'admins' })
function event(sid: string, headers: Record<string, string> = {}, method = 'POST') {
  const req = new IncomingMessage(new Socket()); req.method = method; req.url = '/api/rpc'
  req.headers = { cookie: `__Host-aur-session=${sid}`, ...headers }
  return createEvent(req, new ServerResponse(req))
}
const providers = [ { name: 'sqlite', url: `file:${join(temp, 'test.db')}` }, ...(process.env.TEST_POSTGRESQL_URL ? [{ name: 'postgresql', url: process.env.TEST_POSTGRESQL_URL }] : []) ]
for (const provider of providers) describe(provider.name, () => {
  const db = createDatabase(provider.url)
  beforeAll(async () => {
    execFileSync('cargo', ['run', '-p', 'database', '--example', 'migrate'], { cwd: resolve('..'), env: { ...process.env, DATABASE_URL: provider.url.replace(/^file:/, 'sqlite:') + (provider.name === 'sqlite' ? '?mode=rwc' : '') }, stdio: 'pipe' })
    // Fixture writes are checked independently against both generated clients.
    async function seed(c: SQLiteClient) {
      await c.review_event.deleteMany(); await c.build_attempt.deleteMany(); await c.release_candidate.deleteMany(); await c.build_results.deleteMany(); await c.package_metadata.deleteMany(); await c.web_session.deleteMany(); await c.web_user.deleteMany(); await c.access_audit.deleteMany()
      await c.package_metadata.create({ data: { id: 1, name: 'example', version: '1.0-1', maintainer: 'Alice', last_modified: 1n } })
      await c.release_candidate.create({ data: { id: 'candidate', package_id: 1, fingerprint: 'fingerprint', revision: 'abc123', payload: JSON.stringify(payload), snapshot: JSON.stringify(snapshot), created_at: 1700000000000000n } })
      await c.web_user.create({ data: { id: 'admin', issuer: 'https://issuer.example', subject: 'admin', display_name: 'Admin', admin: true } })
      await c.web_user.create({ data: { id: 'viewer', issuer: 'https://issuer.example', subject: 'viewer', display_name: 'Viewer', can_view: true } })
      await c.build_results.create({ data: { id: 1, package_id: 1n, exit_code: 105, success: false, build_log: '<script>alert(1)</script>', started_at: '2026-11-01 12:00:00' } })
    }
    // Same schema-derived fixture contract, with provider-specific timestamp types excluded.
    if (provider.name === 'sqlite') {
      const client = createSQLiteClient(provider.url)
      await seed(client)
      await client.$disconnect()
    } else {
      const client = createPostgresClient(provider.url)
      await client.review_event.deleteMany(); await client.build_attempt.deleteMany(); await client.release_candidate.deleteMany(); await client.build_results.deleteMany(); await client.package_metadata.deleteMany(); await client.web_session.deleteMany(); await client.web_user.deleteMany(); await client.access_audit.deleteMany()
      await client.package_metadata.create({ data: { id: 1, name: 'example', version: '1.0-1', maintainer: 'Alice', last_modified: 1n } })
      await client.release_candidate.create({ data: { id: 'candidate', package_id: 1, fingerprint: 'fingerprint', revision: 'abc123', payload: JSON.stringify(payload), snapshot: JSON.stringify(snapshot), created_at: 1700000000000000n } })
      await client.web_user.create({ data: { id: 'admin', issuer: 'https://issuer.example', subject: 'admin', display_name: 'Admin', admin: true } })
      await client.web_user.create({ data: { id: 'viewer', issuer: 'https://issuer.example', subject: 'viewer', display_name: 'Viewer', can_view: true } })
      await client.build_results.create({ data: { id: 1, package_id: 1n, exit_code: 105, success: false, build_log: '<script>alert(1)</script>', started_at: new Date('2026-11-01T12:00:00Z') } })
      await client.$disconnect()
    }
  })
  afterAll(() => db.$disconnect())
  it('checks migrated schema, typed reads, redaction and missing records', async () => {
    await assertSchema(db)
    expect((await packages(db)).packages[0]?.build_status).toBe('failed')
    expect(new Date((await history(db, 1)).results[0]!.started_at!).toISOString()).toBe('2026-11-01T12:00:00.000Z')
    expect((await history(db, 1)).results[0]?.finished_at).toBeNull()
    expect((await buildLog(db, 1)).result.build_log).toContain('<script>')
    await expect(history(db, 99)).rejects.toMatchObject({ code: 'NOT_FOUND' })
    const detail = await review(db, 'candidate')
    expect(detail.incomplete).toBe(true)
    expect(detail.source).toBe('https://example.com/source')
    expect(JSON.stringify(detail)).not.toContain('never-expose-this')
    expect(JSON.stringify(detail)).not.toContain('token=hidden')
  })
  it('rolls back all approval state when signing fails', async () => {
    await expect(decide(db, 'candidate', 'admin', 'approve', '', async () => { throw new Error('signing failed') })).rejects.toThrow('signing failed')
    expect((await db.release_candidate.findUnique({ where: { id: 'candidate' } }))?.state).toBe('pending')
    expect(await db.build_attempt.findMany()).toHaveLength(0)
    expect(await db.review_event.findMany()).toHaveLength(0)
  })
  it('validates rejection and atomically accepts one competing approval', async () => {
    await expect(decide(db, 'candidate', 'admin', 'reject', '  ')).rejects.toMatchObject({ code: 'BAD_REQUEST' })
    const results = await Promise.allSettled([decide(db, 'candidate', 'admin', 'approve', '', async () => {}), decide(db, 'candidate', 'admin', 'approve', '', async () => {})])
    expect(results.filter(r => r.status === 'fulfilled')).toHaveLength(1)
    expect(results.find(r => r.status === 'rejected')).toMatchObject({ reason: { code: 'CONFLICT' } })
    const tasks = await db.build_attempt.findMany()
    expect(tasks).toHaveLength(1)
    const task = JSON.parse(tasks[0]!.task)
    expect(task).toMatchObject({ id: 1, candidate_id: 'candidate', revision: 'abc123', name: 'example', version: '2.0-1', env: payload.environment })
    expect(task.upload).toBeUndefined()
    execFileSync('cargo', ['run', '-p', 'database', '--example', 'verify_web_tasks'], { cwd: resolve('..'), env: { ...process.env, DATABASE_URL: provider.url.replace(/^file:/, 'sqlite:') }, stdio: 'pipe' })
    expect(task.object_key).toBe(`builds/${task.attempt}/bundle.tar`)
    expect((await db.review_event.findMany())[0]?.decision).toBe('approved')
    expect((await db.package_metadata.findUnique({ where: { id: 1 } }))?.version).toBe('2.0-1')
    await expect(rebuild(db, 1, 'admin', async () => {})).rejects.toMatchObject({ code: 'CONFLICT' })
  })
  it('enforces permissions, CSRF, hashed/bound sessions and revocation', async () => {
    const { authConfig } = await import('../server/utils/config')
    for (const uid of ['admin', 'viewer']) await db.web_session.create({ data: { id: hash(uid), user_id: uid, csrf: 'csrf', expires_at: now() + 3600n, auth_context: authConfig().context } })
    expect(await identity(event(hash('admin')), db)).toBeNull()
    await db.web_session.create({ data: { id: hash('expired'), user_id: 'admin', csrf: 'csrf', expires_at: now() - 1n, auth_context: authConfig().context } })
    expect(await identity(event('expired'), db)).toBeNull()
    await db.web_session.create({ data: { id: hash('other-config'), user_id: 'admin', csrf: 'csrf', expires_at: now() + 3600n, auth_context: 'different-config' } })
    expect(await identity(event('other-config'), db)).toBeNull()
    const admin = createRouterClient(router, { context: { db, event: event('admin', { origin: 'https://builder.example', 'x-csrf-token': 'csrf' }) } })
    const viewer = createRouterClient(router, { context: { db, event: event('viewer', { origin: 'https://builder.example', 'x-csrf-token': 'csrf' }) } })
    await expect(viewer.buildLog({ id: 1 })).rejects.toMatchObject({ code: 'FORBIDDEN' })
    await expect(viewer.revokeSessions({ id: 'admin' })).rejects.toMatchObject({ code: 'FORBIDDEN' })
    await expect(viewer.users()).rejects.toMatchObject({ code: 'FORBIDDEN' })
    await expect(viewer.rebuild({ id: 1 })).rejects.toMatchObject({ code: 'FORBIDDEN' })
    const unsafe = createRouterClient(router, { context: { db, event: event('admin', { origin: 'https://evil.example', 'x-csrf-token': 'csrf' }) } })
    await expect(unsafe.decide({ id: 'candidate', decision: 'reject', comment: 'test' })).rejects.toMatchObject({ code: 'FORBIDDEN' })
    const noToken = createRouterClient(router, { context: { db, event: event('admin', { origin: 'https://builder.example' }) } })
    await expect(noToken.rebuild({ id: 1 })).rejects.toMatchObject({ code: 'FORBIDDEN' })
    await expect(admin.updateUser({ id: 'admin', can_view: true, can_approve: true, can_rebuild: true, disabled: true })).rejects.toMatchObject({ code: 'BAD_REQUEST' })
    await admin.revokeSessions({ id: 'viewer' })
    expect(await identity(event('viewer'), db)).toBeNull()
    await updateAccess(db, 'admin', { id: 'viewer', can_view: false, can_rebuild: false, can_approve: false, disabled: true })
    expect(await identity(event('viewer'), db)).toBeNull()
    expect(await db.access_audit.findMany()).toHaveLength(2)
    await admin.logout()
    expect((await db.access_audit.findMany()).some(a => a.detail === 'auth.logout')).toBe(true)
    expect(await identity(event('admin'), db)).toBeNull()
  })
  it('paginates large histories and review lists without loading logs or snapshots', async () => {
    const client = provider.name === 'sqlite' ? createSQLiteClient(provider.url) : createPostgresClient(provider.url)
    try {
      await client.package_metadata.create({ data: { id: 2, name: 'zz-pagination', version: '1', maintainer: 'Fixture', last_modified: 1n } })
      await client.build_results.createMany({ data: Array.from({ length: 103 }, (_, i) => ({ id: 200 + i, package_id: 2n, exit_code: 0, success: true, build_log: 'sensitive-large-log' })) })
      await client.release_candidate.createMany({ data: Array.from({ length: 103 }, (_, i) => ({ id: `page-${String(i).padStart(3, '0')}`, package_id: 2, fingerprint: `page-${i}`, revision: 'a'.repeat(40), payload: JSON.stringify({ ...payload, name: 'zz-pagination' }), snapshot: JSON.stringify(snapshot), created_at: BigInt(i) })) })
      const first = await history(db, 2)
      expect(first.results).toHaveLength(100)
      const older = await history(db, 2, first.results.at(-1)!.id)
      expect(older.results.map(row => row.id)).toEqual([202, 201, 200])
      expect(await db.build_results.latestStatuses()).toHaveLength(2)
      const rows = await db.build_results.findMany({ take: 1 })
      expect(rows[0]).not.toHaveProperty('build_log')
      const page = await reviews(db, 'all')
      const next = await reviews(db, 'all', page.at(-1)!.id)
      expect(page).toHaveLength(100)
      expect(new Set([...page, ...next].map(row => row.id)).size).toBe(104)
      const summaries = await db.release_candidate.findMany({ take: 1 })
      expect(summaries[0]).not.toHaveProperty('snapshot')
    } finally { await client.$disconnect() }
  })

})
afterAll(() => rmSync(temp, { recursive: true, force: true }))
