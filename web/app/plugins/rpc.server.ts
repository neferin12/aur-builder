import { createRouterClient } from '@orpc/server'
import { router } from '../../server/rpc/router'
export default defineNuxtPlugin(() => {
  const event = useRequestEvent()
  if (!event) throw new Error('Missing SSR request context')
  return { provide: { rpc: createRouterClient(router, { context: { event } }) } }
})
