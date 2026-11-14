import { createORPCClient } from '@orpc/client'
import { RPCLink } from '@orpc/client/fetch'
import type { RouterClient } from '@orpc/server'
import type { router } from '../../server/rpc/router'
export default defineNuxtPlugin(() => {
  const csrf = useState<string>('csrf', () => '')
  const url = useRequestURL()
  const rpc: RouterClient<typeof router> = createORPCClient(new RPCLink({
    url: `${url.origin}/api/rpc`,
    headers: () => ({ 'x-csrf-token': csrf.value }),
    fetch: (request, init) => fetch(request, { ...init, credentials: 'same-origin' }),
  }))
  return { provide: { rpc } }
})
