import { BlockList, isIP } from 'node:net'
import { createError, getHeader, getRequestIP, setHeader, type H3Event } from 'h3'
// Process-local limits. Only explicitly configured proxy addresses may supply
// forwarded client addresses; walk from the closest hop to resist spoofed prefixes.
let cached: { value: string; list: BlockList } | undefined
function address(value: string) { return value.startsWith('::ffff:') && isIP(value.slice(7)) === 4 ? value.slice(7) : value }
export function validateTrustedProxies() {
  const value = process.env.AB_TRUSTED_PROXIES ?? ''
  if (cached?.value === value) return cached.list
  const list = new BlockList()
  for (const item of value.split(',').map(v => v.trim()).filter(Boolean)) {
    const [ip, prefix, extra] = item.split('/'), version = isIP(ip!)
    if (!version || extra !== undefined) throw new Error('Invalid AB_TRUSTED_PROXIES')
    const family = version === 4 ? 'ipv4' : 'ipv6'
    if (prefix === undefined) list.addAddress(ip!, family)
    else {
      const bits = Number(prefix)
      if (!/^\d+$/.test(prefix) || bits < 0 || bits > (version === 4 ? 32 : 128)) throw new Error('Invalid AB_TRUSTED_PROXIES')
      list.addSubnet(ip!, bits, family)
    }
  }
  cached = { value, list }
  return list
}
export function authenticationClientIp(event: H3Event) {
  const peer = address(getRequestIP(event) ?? 'unknown'), list = validateTrustedProxies()
  const trusted = (ip: string) => !!isIP(ip) && list.check(ip, isIP(ip) === 4 ? 'ipv4' : 'ipv6')
  if (!trusted(peer)) return peer
  const chain = (getHeader(event, 'x-forwarded-for') ?? '').split(',').map(ip => address(ip.trim())).filter(Boolean)
  if (!chain.length || chain.length > 20 || chain.some(ip => !isIP(ip))) return peer
  for (const ip of chain.reverse()) if (!trusted(ip)) return ip
  return chain[chain.length - 1]!
}
const buckets = new Map<string, { tokens: number; at: number }>()
export function limitAuthentication(event: H3Event) {
  const now = Date.now(), key = authenticationClientIp(event)
  for (const [ip, bucket] of buckets) if (now - bucket.at > 600000) buckets.delete(ip)
  const bucket = buckets.get(key) ?? { tokens: 20, at: now }
  bucket.tokens = Math.min(20, bucket.tokens + (now - bucket.at) / 6000)
  bucket.at = now
  if (!buckets.has(key) && buckets.size >= 10000) throw createError({ statusCode: 429, message: 'Too many authentication requests' })
  buckets.set(key, bucket)
  if (bucket.tokens < 1) {
    setHeader(event, 'retry-after', 6)
    throw createError({ statusCode: 429, message: 'Too many authentication requests' })
  }
  bucket.tokens--
}
