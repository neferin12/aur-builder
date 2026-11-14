// Nuxt puts framework scripts in head/bodyAppend. Application body markup must
// never acquire a nonce: CSP must still block a future raw-HTML rendering bug.
export function nonceFrameworkScripts(html: { head: string[]; bodyAppend: string[] }, nonce: string) {
  for (const key of ['head', 'bodyAppend'] as const) {
    html[key] = html[key].map(fragment => fragment.replace(/<script\b/g, `<script nonce="${nonce}"`))
  }
}
