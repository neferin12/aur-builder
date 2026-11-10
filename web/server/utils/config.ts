import { createHash, timingSafeEqual } from 'node:crypto'
export const now = () => BigInt(Math.floor(Date.now() / 1000))
export const hash = (value: string) => createHash('sha256').update(value).digest('hex')
export function equal(a: string, b: string) {
  const left = Buffer.from(a), right = Buffer.from(b)
  return left.length === right.length && timingSafeEqual(left, right)
}
export function required(name: string) {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is required`)
  return value
}
export function secureUrl(value: string, origin = false) {
  const url = new URL(value)
  if (url.protocol !== 'https:' || url.username || url.password || url.hash || (origin && (url.pathname !== '/' || url.search))) throw new Error('Expected an HTTPS URL without credentials')
  return url
}
export function authConfig() {
  const issuer = required('AB_OIDC_ISSUER')
  secureUrl(issuer)
  const origin = secureUrl(required('AB_WEB_URL'), true).origin
  const clientId = required('AB_OIDC_CLIENT_ID'), secret = required('AB_OIDC_CLIENT_SECRET')
  const groupClaim = process.env.AB_OIDC_GROUPS_CLAIM ?? 'groups', adminGroup = required('AB_OIDC_ADMIN_GROUP')
  const scopes = (process.env.AB_OIDC_SCOPES ?? 'profile').split(/\s+/).filter(Boolean)
  const ttl = Number(process.env.AB_SESSION_SECONDS ?? 3600)
  if (!Number.isInteger(ttl) || ttl < 60 || ttl > 86400) throw new Error('AB_SESSION_SECONDS must be between 60 and 86400')
  return { issuer, origin, clientId, secret, groupClaim, adminGroup, scopes, ttl,
    context: hash(JSON.stringify([issuer, origin, clientId, secret, groupClaim, adminGroup, scopes, ttl])) }
}
