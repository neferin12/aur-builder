import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
import { createServer } from 'node:http'
import type { Server } from 'node:http'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { generateKeyPair, exportJWK, SignJWT } from 'jose'
import { createApp, eventHandler, toNodeListener } from 'h3'
import { createSQLiteClient } from '../server/utils/db'
import { callback, login } from '../server/utils/auth'
import { authConfig, hash, now } from '../server/utils/config'
const temp = mkdtempSync(join(tmpdir(), 'aur-oidc-'))
const url = `file:${join(temp, 'auth.db')}`
Object.assign(process.env, { DATABASE_URL: url, AB_WEB_URL: 'https://builder.example', AB_OIDC_ISSUER: 'https://issuer.example', AB_OIDC_CLIENT_ID: 'test', AB_OIDC_CLIENT_SECRET: 'secret', AB_OIDC_ADMIN_GROUP: 'admins' })
const db = createSQLiteClient(url)
let server: Server, origin: string
let claimsOverride: Record<string, unknown> = {}, signingKey: CryptoKey, wrongKey: CryptoKey, publicKey: object
const nonceByCode = new Map<string, string>()
const actualFetch = globalThis.fetch
beforeAll(async () => {
  execFileSync('cargo', ['run', '-p', 'database', '--example', 'migrate'], { cwd: resolve('..'), env: { ...process.env, DATABASE_URL: url.replace('file:', 'sqlite:') + '?mode=rwc' }, stdio: 'pipe' })
  const pair = await generateKeyPair('RS256'); signingKey = pair.privateKey
  publicKey = { ...await exportJWK(pair.publicKey), kid: 'key', use: 'sig', alg: 'RS256' }
  wrongKey = (await generateKeyPair('RS256')).privateKey
  vi.stubGlobal('fetch', async (input: string | URL | Request, init?: RequestInit) => {
    const address = String(input instanceof Request ? input.url : input)
    if (!address.startsWith('https://issuer.example/')) return actualFetch(input, init)
    if (address.endsWith('/.well-known/openid-configuration')) return Response.json({ issuer: 'https://issuer.example', authorization_response_iss_parameter_supported: true, authorization_endpoint: 'https://issuer.example/authorize', token_endpoint: 'https://issuer.example/token', jwks_uri: 'https://issuer.example/keys', response_types_supported: ['code'], subject_types_supported: ['public'], id_token_signing_alg_values_supported: ['RS256'] })
    if (address.endsWith('/keys')) return Response.json({ keys: [publicKey] })
    if (address.endsWith('/token')) {
      const body = new URLSearchParams(String(init?.body)), code = body.get('code')!
      const claims = { iss: 'https://issuer.example', aud: 'test', sub: 'alice', name: 'Alice', groups: ['admins'], nonce: nonceByCode.get(code), iat: Number(now()), exp: Number(now()) + 300, ...claimsOverride }
      const token = await new SignJWT(claims).setProtectedHeader({ alg: 'RS256', kid: 'key' }).sign(code === 'wrong-signature' ? wrongKey : signingKey)
      return Response.json({ access_token: 'access', token_type: 'Bearer', id_token: token })
    }
    throw new Error(`Unexpected OIDC request ${address}`)
  })
  const app = createApp().use('/auth/login', eventHandler(login)).use('/auth/callback', eventHandler(callback))
  server = createServer(toNodeListener(app))
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve))
  const addr = server.address(); if (!addr || typeof addr === 'string') throw new Error('No address')
  origin = `http://127.0.0.1:${addr.port}`
})
afterAll(async () => { vi.unstubAllGlobals(); await new Promise<void>(resolve => server.close(() => resolve())); await db.$disconnect(); const { database } = await import('../server/utils/db'); await database().$disconnect(); rmSync(temp, { recursive: true, force: true }) })
async function start(code = 'valid') {
  const response = await actualFetch(`${origin}/auth/login`, { redirect: 'manual' })
  expect(response.status).toBe(303)
  const authorization = new URL(response.headers.get('location')!)
  nonceByCode.set(code, authorization.searchParams.get('nonce')!)
  return { state: authorization.searchParams.get('state')!, cookie: response.headers.get('set-cookie')!.split(';')[0]!, code }
}
async function finish(record: Awaited<ReturnType<typeof start>>, cookie = record.cookie, issuer = 'https://issuer.example') {
  return actualFetch(`${origin}/auth/callback?${new URLSearchParams({ code: record.code, state: record.state, iss: issuer })}`, { redirect: 'manual', headers: { cookie } })
}
describe('OIDC code grant with signed mock provider responses', () => {
  it('creates a hashed, bound group-admin session and consumes state once', async () => {
    claimsOverride = {}
    const record = await start()
    const response = await finish(record)
    expect(response.status).toBe(303)
    const cookies = response.headers.getSetCookie()
    expect(cookies.join(';')).toContain('HttpOnly')
    expect(cookies.join(';')).toContain('Secure')
    const sid = cookies.find(c => c.startsWith('__Host-aur-session='))!.split(';')[0]!.split('=')[1]!
    expect(await db.web_session.findUnique({ where: { id: sid } })).toBeNull()
    const session = await db.web_session.findUnique({ where: { id: hash(sid) } })
    expect(session?.auth_context).toBe(authConfig().context)
    expect((await db.web_user.findUnique({ where: { id: session!.user_id } }))?.admin).toBe(true)
    expect((await finish(record)).status).toBe(400)
  })
  it('revokes prior sessions when the OIDC administrator group changes', async () => {
    claimsOverride = {}
    const first = await finish(await start())
    const sid = first.headers.getSetCookie().find(c => c.startsWith('__Host-aur-session='))!.split(';')[0]!.split('=')[1]!
    claimsOverride = { groups: [] }
    expect((await finish(await start())).status).toBe(303)
    expect(await db.web_session.findUnique({ where: { id: hash(sid) } })).toBeNull()
    expect((await db.web_user.findUnique({ where: { issuer_subject: { issuer: 'https://issuer.example', subject: 'alice' } } }))?.admin).toBe(false)
  })
  it.each([
    ['iss', 'https://evil.example'], ['aud', 'another-client'], ['nonce', 'wrong'], ['exp', 1], ['iat', 9999999999], ['azp', 'another-client'], ['at_hash', 'invalid-hash'],
  ])('rejects invalid %s', async (claim, value) => {
    claimsOverride = { [claim]: value }
    expect((await finish(await start())).status).toBe(401)
  })
  it('rejects a mismatched authorization response issuer', async () => {
    claimsOverride = {}
    expect((await finish(await start(), undefined, 'https://evil.example')).status).toBe(401)
  })
  it('rejects forged signatures, browser binding mismatch and changed config', async () => {
    claimsOverride = {}
    expect((await finish(await start('wrong-signature'))).status).toBe(401)
    const record = await start()
    expect((await finish(record, '__Host-aur-login=wrong')).status).toBe(400)
    process.env.AB_OIDC_ADMIN_GROUP = 'other-admins'
    expect((await finish(record)).status).toBe(400)
    process.env.AB_OIDC_ADMIN_GROUP = 'admins'
  })
})
