// Compiled by Nuxt typecheck: these assertions must continue to fail as marked.
import type { RouterClient } from '@orpc/server'
import type { router } from '../../server/rpc/router'
import type { PrismaClient } from '../../generated/sqlite/index.js'
export function checkTypes(client: RouterClient<typeof router>, db: PrismaClient) {
  // @ts-expect-error oRPC infers numeric package IDs from the input schema.
  client.history({ id: '1' })
  // @ts-expect-error oRPC rejects unknown decisions.
  client.decide({ id: 'candidate', decision: 'publish', comment: '' })
  // @ts-expect-error Prisma queries derive column names from the schema.
  db.package_metadata.findMany({ where: { nonexistent: true } })
  // @ts-expect-error Prisma derives scalar types from the schema.
  db.package_metadata.findUnique({ where: { id: '1' } })
  client.history({ id: 1 }).then(result => {
    const version: string = result.package.version
    // @ts-expect-error Output types are inferred through the oRPC router.
    const wrong: number = result.package.version
    return { version, wrong }
  })
}
