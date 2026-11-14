import { randomBytes } from 'node:crypto'
export default defineEventHandler(event => {
  const nonce = randomBytes(24).toString('base64')
  event.context.cspNonce = nonce
  setResponseHeaders(event, {
    'cache-control': 'no-store', 'x-content-type-options': 'nosniff', 'x-frame-options': 'DENY',
    'referrer-policy': event.path.startsWith('/auth/') ? 'no-referrer' : 'same-origin',
    'permissions-policy': 'camera=(), microphone=(), geolocation=()',
    'cross-origin-opener-policy': 'same-origin',
    'strict-transport-security': 'max-age=31536000',
    'content-security-policy': `default-src 'none'; script-src 'self' 'nonce-${nonce}'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'; object-src 'none'`,
  })
})
