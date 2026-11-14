import { RPCHandler } from '@orpc/server/fetch'
import { router } from '../../rpc/router'
const handler = new RPCHandler(router)
export default defineEventHandler(async event => {
  const { response } = await handler.handle(toWebRequest(event), { prefix: '/api/rpc', context: { event } })
  return response ?? new Response('Not found', { status: 404 })
})
