import { describe, expect, it, vi } from 'vitest'
import { IncomingMessage, ServerResponse } from 'node:http'
import { Socket } from 'node:net'
import { createEvent } from 'h3'
import { limitAuthentication, authenticationClientIp } from '../server/utils/rate-limit'
import { sourceDiffs } from '../server/utils/diffs'
import { createRequire } from 'node:module'

it('blocks prototype keys from both deepmerge inputs, including null-prototype targets', () => {
  const require = createRequire(import.meta.url)
  const poison = JSON.parse('{"__proto__":{"admin":true},"constructor":{"prototype":{"admin":true}},"prototype":{"admin":true},"safe":1}')
  for (const merge of [require('deepmerge'), require('deepmerge/dist/umd.js')]) {
    for (const result of [merge(Object.create(null), poison), merge(poison, {}), merge({}, poison)]) {
      expect(Object.getPrototypeOf(result)).toBe(Object.prototype)
      expect(result.admin).toBeUndefined()
      expect(Object.hasOwn(result, '__proto__')).toBe(false)
      expect(Object.hasOwn(result, 'constructor')).toBe(false)
      expect(Object.hasOwn(result, 'prototype')).toBe(false)
      expect(result.safe).toBe(1)
    }
  }
})
describe('security budgets', () => {
  it('throttles repeated authentication requests and refills over time', () => {
    vi.useFakeTimers()
    try {
      const req = new IncomingMessage(new Socket())
      const event = createEvent(req, new ServerResponse(req))
      for (let i = 0; i < 20; i++) limitAuthentication(event)
      expect(() => limitAuthentication(event)).toThrow('Too many')
      vi.advanceTimersByTime(6000)
      expect(() => limitAuthentication(event)).not.toThrow()
    } finally { vi.useRealTimers() }
  })
  it('limits cumulative diff content even when each file is small', () => {
    const files = Array.from({ length: 10 }, (_, i) => ({ path: `file-${i}`, content: 'x'.repeat(60000), size: 1, hash: String(i), mode: 33188 }))
    const diffs = sourceDiffs({ complete: true, files: [] }, { complete: true, files })
    expect(diffs.filter(file => file.newText !== null)).toHaveLength(4)
    expect(diffs.filter(file => file.newText === null).every(file => file.message.includes('Review outside'))).toBe(true)
  })
  it('omits oversized diff text with an explicit review warning', () => {
    const content = 'x'.repeat(200000)
    const diffs = sourceDiffs({ complete: true, files: [] }, { complete: true, files: [{ path: 'PKGBUILD', content, size: content.length, hash: 'h', mode: 33188 }] })
    expect(JSON.stringify(diffs)).not.toContain(content)
    expect(diffs[0]?.message).toContain('Review outside the UI')
  })
})

describe('authentication proxy boundary', () => {
  const event = (peer: string, forwarded: string) => {
    const socket = new Socket()
    Object.defineProperty(socket, 'remoteAddress', { value: peer })
    const req = new IncomingMessage(socket)
    req.headers = { 'x-forwarded-for': forwarded }
    return createEvent(req, new ServerResponse(req))
  }
  it('ignores spoofed forwarded addresses from untrusted peers', () => {
    vi.stubEnv('AB_TRUSTED_PROXIES', '127.0.0.1/32')
    try { expect(authenticationClientIp(event('198.51.100.2', '203.0.113.9'))).toBe('198.51.100.2') }
    finally { vi.unstubAllEnvs() }
  })
  it('uses the closest untrusted hop and handles mapped socket addresses', () => {
    vi.stubEnv('AB_TRUSTED_PROXIES', '127.0.0.1/32,10.0.0.2')
    try {
      expect(authenticationClientIp(event('::ffff:127.0.0.1', 'attacker-input,203.0.113.7'))).toBe('127.0.0.1')
      expect(authenticationClientIp(event('::ffff:127.0.0.1', '192.0.2.99,203.0.113.7,10.0.0.2'))).toBe('203.0.113.7')
    } finally { vi.unstubAllEnvs() }
  })
})

it('nonces framework scripts without authorizing scripts in page content', async () => {
  const { nonceFrameworkScripts } = await import('../server/utils/csp')
  const html = { head: ['<script src="/_nuxt/app.js"></script>'], bodyAppend: ['<script>window.__NUXT__={}</script>'], body: ['<script>attacker()</script>'] }
  nonceFrameworkScripts(html, 'server-generated-nonce')
  expect(html.head[0]).toContain('nonce="server-generated-nonce"')
  expect(html.bodyAppend[0]).toContain('nonce="server-generated-nonce"')
  expect(html.body[0]).toBe('<script>attacker()</script>')
})
