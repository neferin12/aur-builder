import { nonceFrameworkScripts } from '../utils/csp'
export default defineNitroPlugin(nitro => {
  nitro.hooks.hook('render:html', (html, { event }) => {
    const nonce = event.context.cspNonce as string | undefined
    if (nonce) nonceFrameworkScripts(html, nonce)
  })
})
