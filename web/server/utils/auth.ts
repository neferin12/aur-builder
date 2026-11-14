import { limitAuthentication } from './rate-limit'
import { createHash, randomUUID } from 'node:crypto'
import * as oidc from 'openid-client'
import { z } from 'zod'
import { createError, deleteCookie, getCookie, getQuery, getRequestURL, sendRedirect, setCookie, type H3Event } from 'h3'
import { authConfig, equal, hash, now, secureUrl } from './config'
import { database, transaction, type Database } from './db'
const sessionCookie = '__Host-aur-session', loginCookie = '__Host-aur-login'
const cookieOptions = { httpOnly: true, secure: true, sameSite: 'lax' as const, path: '/' }
const loginSchema = z.object({ nonce: z.string(), verifier: z.string(), browser: z.string(), context: z.string() })
let cached: { context: string; expires: number; client: oidc.Configuration } | undefined
// No redirects to unvalidated endpoints, including during discovery and JWKS fetches.
const safeFetch: oidc.CustomFetch = (input, init) => {
  secureUrl(String(input))
  return fetch(input, { ...init, body: init.body instanceof Uint8Array ? new Uint8Array(init.body).buffer : init.body, redirect: 'error', signal: AbortSignal.timeout(30000) })
}
export async function oidcClient() {
  const config = authConfig()
  if (cached?.context === config.context && cached.expires > Date.now()) return cached.client
  const client = await oidc.discovery(new URL(config.issuer), config.clientId, config.secret, oidc.ClientSecretBasic(config.secret), { [oidc.customFetch]: safeFetch, timeout: 30 })
  const metadata = client.serverMetadata()
  for (const endpoint of [metadata.authorization_endpoint, metadata.token_endpoint, metadata.jwks_uri]) {
    if (!endpoint) throw new Error('OIDC metadata is incomplete')
    secureUrl(endpoint)
  }
  oidc.enableNonRepudiationChecks(client)
  cached = { context: config.context, expires: Date.now() + 300000, client }
  return client
}
export async function identity(event: H3Event, db: Database = database()) {
  const sid = getCookie(event, sessionCookie)
  if (!sid) return null
  const session = await db.web_session.findFirst({ where: { id: hash(sid), expires_at: { gt: now() }, auth_context: authConfig().context } })
  if (!session) return null
  const user = await db.web_user.findUnique({ where: { id: session.user_id } })
  if (!user || user.disabled) return null
  return { user, csrf: session.csrf, sessionId: sid }
}
export async function login(event: H3Event) {
  limitAuthentication(event)
  const config = authConfig(), client = await oidcClient(), db = database()
  const verifier = oidc.randomPKCECodeVerifier(), nonce = oidc.randomNonce(), state = oidc.randomState(), browser = randomUUID()
  await db.oidc_login.create({ data: { id: state, payload: JSON.stringify({ verifier, nonce, browser, context: config.context }), expires_at: now() + 600n } })
  setCookie(event, loginCookie, browser, { ...cookieOptions, maxAge: 600 })
  const url = oidc.buildAuthorizationUrl(client, { redirect_uri: `${config.origin}/auth/callback`, scope: ['openid', ...config.scopes].join(' '), state, nonce, code_challenge: await oidc.calculatePKCECodeChallenge(verifier), code_challenge_method: 'S256' })
  return sendRedirect(event, url.href, 303)
}
export async function callback(event: H3Event) {
  limitAuthentication(event)
  try { return await completeCallback(event) }
  catch (error) {
    // Do not retain authorization codes, state, cookies, provider responses, or
    // exception text. Failure records deliberately identify no unverified subject.
    await database().access_audit.create({ data: { id: randomUUID(), actor: 'anonymous', target: 'authentication', detail: 'auth.login_failed', created_at: now() } })
    throw error
  }
}
async function completeCallback(event: H3Event) {
  const config = authConfig(), db = database(), query = getQuery(event)
  if (typeof query.state !== 'string' || typeof query.code !== 'string') throw createError({ statusCode: 400, message: 'Invalid OIDC response' })
  const row = await db.oidc_login.findFirst({ where: { id: query.state, expires_at: { gt: now() } } })
  if (!row) throw createError({ statusCode: 400, message: 'Login expired or already consumed' })
  const record = loginSchema.parse(JSON.parse(row.payload))
  if (!equal(getCookie(event, loginCookie) ?? '', record.browser) || record.context !== config.context) throw createError({ statusCode: 400, message: 'Login context changed' })
  const consumed = await db.oidc_login.deleteMany({ where: { id: query.state, expires_at: { gt: now() } } })
  if (consumed.count !== 1) throw createError({ statusCode: 400, message: 'Login already consumed' })
  const client = await oidcClient()
  let claims: oidc.IDToken
  try {
    const response = new URL(`${config.origin}/auth/callback`)
    // Keep RFC 9207's issuer parameter (and other provider response fields).
    // Keycloak advertises issuer validation, so the OIDC client also needs `iss`
    // when validating the response, even when state/code are otherwise valid.
    response.search = getRequestURL(event).search
    const tokens = await oidc.authorizationCodeGrant(client, response, { pkceCodeVerifier: record.verifier, expectedNonce: record.nonce, expectedState: query.state, idTokenExpected: true })
    claims = tokens.claims()!
    if (!claims || (claims.azp && claims.azp !== config.clientId) || claims.iat > Number(now()) + 60) throw new Error('Invalid ID token')
    // Code-grant validation does not check optional at_hash; preserve the existing
    // binding between the signed ID token and the returned access token.
    if (claims.at_hash !== undefined) {
      const header = z.object({ alg: z.string() }).parse(JSON.parse(Buffer.from(tokens.id_token!.split('.')[0]!, 'base64url').toString()))
      const bits = /^(?:RS|PS|ES|HS)(256|384|512)$/.exec(header.alg)?.[1]
      if (!bits || typeof claims.at_hash !== 'string') throw new Error('Unsupported access token hash')
      const digest = createHash(`sha${bits}`).update(tokens.access_token).digest()
      if (!equal(claims.at_hash, digest.subarray(0, digest.length / 2).toString('base64url'))) throw new Error('Access token hash mismatch')
    }
  } catch { throw createError({ statusCode: 401, message: 'OIDC token validation failed' }) }
  const groups = claims[config.groupClaim]
  const admin = Array.isArray(groups) && groups.includes(config.adminGroup)
  const subject = claims.sub, name = typeof claims.name === 'string' ? claims.name : subject
  const ttl = admin ? Math.min(config.ttl, 300) : config.ttl
  const sid = randomUUID(), csrf = randomUUID()
  await transaction(db, async tx => {
    const existing = await tx.web_user.findUnique({ where: { issuer_subject: { issuer: config.issuer, subject } } })
    if (existing) {
      await tx.web_user.update({ where: { id: existing.id }, data: { display_name: name } })
      if (existing.admin !== admin) await tx.web_session.deleteMany({ where: { user_id: existing.id } })
    }
    const user = await tx.web_user.upsert({ where: { issuer_subject: { issuer: config.issuer, subject } }, create: { id: randomUUID(), issuer: config.issuer, subject, display_name: name, admin }, update: { display_name: name, admin } })
    if (user.disabled) throw createError({ statusCode: 403, message: 'Account disabled' })
    const old = getCookie(event, sessionCookie)
    if (old) await tx.web_session.deleteMany({ where: { id: hash(old) } })
    await tx.web_session.deleteMany({ where: { expires_at: { lte: now() } } })
    await tx.web_session.create({ data: { id: hash(sid), user_id: user.id, csrf, expires_at: now() + BigInt(ttl), auth_context: config.context } })
    await tx.access_audit.create({ data: { id: randomUUID(), actor: user.id, target: user.id, detail: 'auth.login', created_at: now() } })
  })
  setCookie(event, sessionCookie, sid, { ...cookieOptions, maxAge: ttl })
  deleteCookie(event, loginCookie, cookieOptions)
  return sendRedirect(event, '/', 303)
}
export async function logout(event: H3Event, db: Database = database()) {
  const sid = getCookie(event, sessionCookie)
  if (sid) await transaction(db, async tx => {
    const session = await tx.web_session.findFirst({ where: { id: hash(sid) } })
    await tx.web_session.deleteMany({ where: { id: hash(sid) } })
    if (session) await tx.access_audit.create({ data: { id: randomUUID(), actor: session.user_id, target: session.user_id, detail: 'auth.logout', created_at: now() } })
  })
  deleteCookie(event, sessionCookie, cookieOptions)
}
