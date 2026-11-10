export default defineEventHandler(event => {
  setResponseHeaders(event, {
    'cache-control': 'no-store', 'x-content-type-options': 'nosniff', 'x-frame-options': 'DENY',
    'referrer-policy': event.path.startsWith('/auth/') ? 'no-referrer' : 'same-origin',
    'strict-transport-security': 'max-age=31536000',
    'content-security-policy': "frame-ancestors 'none'; base-uri 'self'; form-action 'self'; object-src 'none'",
  })
})
