// Isolated HTTPS deployment with a real signed OIDC provider and migrated database.
// Test certificates and signing keys are never used by the application.
import { createServer } from 'node:https'
import { request as httpRequest } from 'node:http'
import { readFileSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawn, execFileSync } from 'node:child_process'
import { generateKeyPair, exportJWK, SignJWT } from 'jose'
import { PrismaClient } from '../generated/sqlite/index.js'
import { PrismaBetterSQLite3 } from '@prisma/adapter-better-sqlite3'
const temp = mkdtempSync(join(tmpdir(), 'aur-e2e-')), database = `sqlite:${join(temp, 'db.sqlite')}?mode=rwc`
const origin = 'https://localhost:3443'
execFileSync('cargo', ['run', '-p', 'database', '--example', 'migrate'], { cwd: resolve('..'), env: { ...process.env, DATABASE_URL: database }, stdio: 'inherit' })
const db = new PrismaClient({ adapter: new PrismaBetterSQLite3({ url: `file:${join(temp, 'db.sqlite')}` }) })
await db.package_metadata.create({ data: { id: 1, name: 'example', version: '1.0-1', maintainer: 'Alice', last_modified: 1n } })
const payload = { name: 'example', version: '2.0-1', maintainer: 'Alice', last_modified: 2, source: null, subfolder: null, options: null, environment: [{ name: 'SECRET', value: 'never-expose-this' }] }
for (const id of ['approve-me', 'reject-me']) await db.release_candidate.create({ data: { id, package_id: 1, fingerprint: id, revision: 'abc123', payload: JSON.stringify(payload), snapshot: JSON.stringify({ complete: true, files: [{ path: 'PKGBUILD', content: 'pkgver=2\n', hash: 'hash', mode: 33188, size: 9 }] }), created_at: BigInt(Date.now()) * 1000n } })
await db.build_results.create({ data: { id: 1, package_id: 1n, exit_code: 105, success: false, build_log: '<script>window.pwned=true</script>\nBuild failed' } })
const { privateKey, publicKey } = await generateKeyPair('RS256')
const jwk = { ...await exportJWK(publicKey), kid: 'test', alg: 'RS256', use: 'sig' }
const codes = new Map()
let child, stopping = false
const server = createServer({ key: readFileSync('tests/certs/localhost-key.pem'), cert: readFileSync('tests/certs/localhost.pem') }, async (req, res) => {
  const url = new URL(req.url, origin)
  const json = value => { res.writeHead(200, { 'content-type': 'application/json' }); res.end(JSON.stringify(value)) }
  if (url.pathname === '/oidc/.well-known/openid-configuration') return json({ issuer: `${origin}/oidc`, authorization_endpoint: `${origin}/oidc/authorize`, token_endpoint: `${origin}/oidc/token`, jwks_uri: `${origin}/oidc/keys`, response_types_supported: ['code'], subject_types_supported: ['public'], id_token_signing_alg_values_supported: ['RS256'] })
  if (url.pathname === '/oidc/keys') return json({ keys: [jwk] })
  if (url.pathname === '/oidc/authorize') {
    const code = crypto.randomUUID(); codes.set(code, url.searchParams.get('nonce'))
    const target = new URL('/auth/callback', origin); target.searchParams.set('code', code); target.searchParams.set('state', url.searchParams.get('state'))
    res.writeHead(302, { location: target.href }); return res.end()
  }
  if (url.pathname === '/oidc/token') {
    let body = ''; for await (const chunk of req) body += chunk
    const code = new URLSearchParams(body).get('code'), nonce = codes.get(code); codes.delete(code)
    if (!nonce) { res.writeHead(400); return res.end() }
    const token = await new SignJWT({ nonce, name: 'Test Admin', groups: ['admins'] }).setProtectedHeader({ alg: 'RS256', kid: 'test' }).setIssuer(`${origin}/oidc`).setAudience('test').setSubject('admin').setIssuedAt().setExpirationTime('5m').sign(privateKey)
    return json({ access_token: 'access', token_type: 'Bearer', id_token: token })
  }
  // Test fixture control is restricted to this harness, never a Nuxt endpoint.
  if (url.pathname === '/_fixture/finish') { await db.build_attempt.updateMany({ data: { state: 'finished' } }); res.writeHead(204); return res.end() }
  const upstream = httpRequest({ hostname: '127.0.0.1', port: 3444, path: req.url, method: req.method, headers: req.headers }, response => { res.writeHead(response.statusCode, response.headers); response.pipe(res) })
  upstream.on('error', () => { res.writeHead(503); res.end('Starting') }); req.pipe(upstream)
})
await new Promise(resolve => server.listen(3443, '127.0.0.1', resolve))
child = spawn(process.execPath, ['.output/server/index.mjs'], { stdio: 'inherit', env: {
  ...process.env, PORT: '3444', HOST: '127.0.0.1', DATABASE_URL: database,
  NODE_EXTRA_CA_CERTS: resolve('tests/certs/localhost.pem'),
  AB_WEB_URL: origin, AB_OIDC_ISSUER: `${origin}/oidc`, AB_OIDC_CLIENT_ID: 'test', AB_OIDC_CLIENT_SECRET: 'secret', AB_OIDC_ADMIN_GROUP: 'admins',
  AB_S3_ENDPOINT: 'https://storage.example', AB_S3_BUCKET: 'private', AB_S3_ACCESS_KEY: 'test', AB_S3_SECRET_KEY: 'secret',
} })
async function cleanup() { if (stopping) return; stopping = true; child?.kill('SIGTERM'); server.close(); await db.$disconnect(); rmSync(temp, { recursive: true, force: true }); process.exit(0) }
process.on('SIGTERM', cleanup); process.on('SIGINT', cleanup)
child.on('exit', code => { if (!stopping) { console.error(`Nuxt exited: ${code}`); cleanup() } })
