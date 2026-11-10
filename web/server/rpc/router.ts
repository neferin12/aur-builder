import { os, ORPCError } from '@orpc/server'
import type { H3Event } from 'h3'
import { getHeader } from 'h3'
import { z } from 'zod'
import { identity, logout } from '../utils/auth'
import { authConfig, equal } from '../utils/config'
import { database, type Database } from '../utils/db'
import { allows, type Permission } from '../utils/permissions'
import * as repository from '../utils/repository'
export type Context = { event: H3Event; db?: Database }
const base = os.$context<Context>()
function authorized(permission: Permission, mutation = false) {
  return base.use(async ({ context, next }) => {
    const db = context.db ?? database(), session = await identity(context.event, db)
    if (!session) throw new ORPCError('UNAUTHORIZED')
    if (!allows(session.user, permission)) throw new ORPCError('FORBIDDEN')
    if (mutation && (context.event.method !== 'POST' || getHeader(context.event, 'origin') !== authConfig().origin || !equal(getHeader(context.event, 'x-csrf-token') ?? '', session.csrf))) throw new ORPCError('FORBIDDEN', { message: 'Invalid CSRF token or origin' })
    return next({ context: { ...context, db, session } })
  })
}
const id = z.object({ id: z.number().int().positive().max(2147483647) })
const candidateId = z.object({ id: z.string().min(1).max(200) })
export const router = {
  session: authorized('session').handler(({ context }) => ({ user: context.session.user, csrf: context.session.csrf })),
  packages: authorized('view').handler(({ context }) => repository.packages(context.db)),
  history: authorized('view').input(id).handler(({ context, input }) => repository.history(context.db, input.id)),
  buildLog: authorized('view').input(id).handler(({ context, input }) => repository.buildLog(context.db, input.id)),
  reviews: authorized('view').input(z.object({ status: z.enum(['pending', 'approved', 'rejected', 'superseded', 'all']).default('pending') })).handler(({ context, input }) => repository.reviews(context.db, input.status)),
  review: authorized('view').input(candidateId).handler(({ context, input }) => repository.review(context.db, input.id)),
  decide: authorized('approve', true).input(candidateId.extend({ decision: z.enum(['approve', 'reject']), comment: z.string().max(16000).default('') })).handler(({ context, input }) => repository.decide(context.db, input.id, context.session.user.id, input.decision, input.comment)),
  rebuild: authorized('rebuild', true).input(id).handler(({ context, input }) => repository.rebuild(context.db, input.id, context.session.user.id)),
  users: authorized('admin').handler(({ context }) => context.db.web_user.findMany({ orderBy: { display_name: 'asc' } })),
  updateUser: authorized('admin', true).input(z.object({ id: z.string().min(1), can_view: z.boolean(), can_rebuild: z.boolean(), can_approve: z.boolean(), disabled: z.boolean() })).handler(({ context, input }) => repository.updateAccess(context.db, context.session.user.id, input)),
  logout: authorized('session', true).handler(async ({ context }) => { await logout(context.event, context.db); return { redirect: '/auth/signed-out' } }),
}
