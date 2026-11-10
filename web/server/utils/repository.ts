import { randomUUID } from 'node:crypto'
import { ORPCError } from '@orpc/server'
import type { release_candidate } from '../../generated/sqlite/index.js'
import { transaction, type Database, type Transaction } from './db'
import { payloadSchema, snapshotSchema, taskSchema } from './payloads'
import { now } from './config'
import { sourceDiffs } from './diffs'
import { validateUploadGrant } from './storage'
const missing = () => new ORPCError('NOT_FOUND')
const conflict = () => new ORPCError('CONFLICT', { message: 'This release is no longer pending or has no approved revision.' })
function summary(candidate: release_candidate) {
  const payload = payloadSchema.parse(JSON.parse(candidate.payload))
  return { id: candidate.id, name: payload.name, version: payload.version, revision: candidate.revision, state: candidate.state, created_at: Number(candidate.created_at / 1000000n) }
}
export function publicSource(value: string | null) {
  if (!value) return null
  try { const url = new URL(value); url.username = ''; url.password = ''; url.search = ''; url.hash = ''; return url.href } catch { return '[private source]' }
}
export async function packages(db: Database) {
  const [rows, results, attempts] = await Promise.all([
    db.package_metadata.findMany({ orderBy: { name: 'asc' } }),
    db.build_results.latestStatuses(),
    db.build_attempt.findMany({ where: { state: { in: ['queued', 'dispatched', 'building'] } }, orderBy: [{ created_at: 'asc' }, { attempt: 'asc' }] }),
  ])
  const statuses = new Map<number, boolean>()
  for (const result of results) if (!statuses.has(Number(result.package_id))) statuses.set(Number(result.package_id), result.success)
  return {
    packages: rows.map(p => ({ ...p, last_modified: Number(p.last_modified), source: publicSource(p.source), build_status: statuses.has(p.id) ? statuses.get(p.id) ? 'success' : 'failed' : 'unbuilt' })),
    activity: attempts.map(a => { const t = taskSchema.parse(JSON.parse(a.task)); return { id: t.id, name: t.name, version: t.version, state: a.state } }),
  }
}
function timestamp(value: string | Date | null) { return value instanceof Date ? value.toISOString() : value ? `${value.replace(' ', 'T').replace(/Z$/, '')}Z` : null }
export async function history(db: Database, id: number) {
  const pkg = await db.package_metadata.findUnique({ where: { id } })
  if (!pkg) throw missing()
  const results = await db.build_results.findMany({ where: { package_id: BigInt(id) }, orderBy: { id: 'desc' } })
  return { package: { ...pkg, last_modified: Number(pkg.last_modified), source: publicSource(pkg.source) }, results: results.map(r => ({ id: r.id, success: r.success, exit_code: r.exit_code, version: r.version, started_at: timestamp(r.started_at), finished_at: timestamp(r.finished_at) })) }
}
export async function buildLog(db: Database, id: number) {
  const result = await db.build_results.findUnique({ where: { id } })
  if (!result) throw missing()
  const pkg = await db.package_metadata.findUnique({ where: { id: Number(result.package_id) } })
  if (!pkg) throw missing()
  return { package: { id: pkg.id, name: pkg.name }, result: { ...result, package_id: Number(result.package_id), started_at: timestamp(result.started_at), finished_at: timestamp(result.finished_at) } }
}
export async function reviews(db: Database, status: string) {
  return (await db.release_candidate.findMany({ where: status === 'all' ? {} : { state: status }, orderBy: [{ created_at: 'desc' }, { id: 'desc' }] })).map(summary)
}
export async function review(db: Database, id: string) {
  const candidate = await db.release_candidate.findUnique({ where: { id } })
  if (!candidate) throw missing()
  const baseline = candidate.baseline ? await db.release_candidate.findUnique({ where: { id: candidate.baseline } }) : null
  if (candidate.baseline && !baseline) throw new Error('Missing review baseline')
  const payload = payloadSchema.parse(JSON.parse(candidate.payload))
  const previous = baseline ? payloadSchema.parse(JSON.parse(baseline.payload)) : null
  const snapshot = snapshotSchema.parse(JSON.parse(candidate.snapshot))
  const events = await db.review_event.findMany({ where: { candidate_id: id }, orderBy: { created_at: 'asc' } })
  const ai = await db.ai_review.findUnique({ where: { candidate_id: id } })
  return { candidate: summary(candidate), baseline_revision: baseline?.revision ?? null,
    source: publicSource(payload.source), subfolder: payload.subfolder, options: payload.options,
    incomplete: !snapshot.complete, settings_changed: !!previous && JSON.stringify([previous.options, previous.subfolder, previous.environment, previous.source]) !== JSON.stringify([payload.options, payload.subfolder, payload.environment, payload.source]),
    diffs: sourceDiffs(baseline ? snapshotSchema.parse(JSON.parse(baseline.snapshot)) : { complete: true, files: [] }, snapshot),
    events: events.map(e => ({ actor: e.actor, decision: e.decision, comment: e.comment, created_at: Number(e.created_at) })),
    ai: ai ? JSON.parse(ai.result) as unknown : null,
  }
}
async function enqueue(tx: Transaction, candidate: release_candidate, sign: typeof validateUploadGrant) {
  const p = payloadSchema.parse(JSON.parse(candidate.payload)), attempt = randomUUID(), objectKey = `builds/${attempt}/bundle.tar`
  await sign(objectKey)
  const task = taskSchema.parse({ id: candidate.package_id, candidate_id: candidate.id, revision: candidate.revision, name: p.name, version: p.version, source: p.source, subfolder: p.subfolder, options: p.options, env: p.environment, attempt, object_key: objectKey })
  await tx.build_attempt.create({ data: { attempt, package_id: candidate.package_id, object_key: objectKey, task: JSON.stringify(task), created_at: BigInt(Date.now()) * 1000000n } })
}
export async function decide(db: Database, id: string, actor: string, decision: 'approve' | 'reject', comment: string, sign = validateUploadGrant) {
  if (Buffer.byteLength(comment) > 16000 || (decision === 'reject' && !comment.trim())) throw new ORPCError('BAD_REQUEST', { message: 'Rejection requires a comment (maximum 16000 bytes).' })
  return transaction(db, async tx => {
    const c = await tx.release_candidate.findUnique({ where: { id } })
    if (!c) throw missing()
    // Acquire the same package row lock used by the scheduler's source observations.
    await tx.package_metadata.update({ where: { id: c.package_id }, data: { last_modified: { increment: 0 } } })
    const state = decision === 'approve' ? 'approved' : 'rejected'
    const changed = await tx.release_candidate.updateMany({ where: { id, state: 'pending' }, data: { state } })
    if (changed.count !== 1) throw conflict()
    if (decision === 'approve') {
      await enqueue(tx, c, sign)
      const p = payloadSchema.parse(JSON.parse(c.payload))
      await tx.package_metadata.update({ where: { id: c.package_id }, data: { version: p.version, source: p.source, subfolder: p.subfolder, last_modified: BigInt(p.last_modified) } })
    }
    await tx.review_event.create({ data: { id: randomUUID(), candidate_id: id, actor, decision: state, comment, created_at: now() } })
    return { state }
  })
}
export async function rebuild(db: Database, id: number, actor: string, sign = validateUploadGrant) {
  return transaction(db, async tx => {
    const p = await tx.package_metadata.findUnique({ where: { id } })
    if (!p) throw missing()
    await tx.package_metadata.update({ where: { id }, data: { last_modified: { increment: 0 } } })
    const c = await tx.release_candidate.findFirst({ where: { package_id: id, state: 'approved' }, orderBy: [{ created_at: 'desc' }, { id: 'desc' }] })
    if (!c) throw conflict()
    // Avoid replacing an attempt already queued/running for the same package.
    const active = await tx.build_attempt.findFirst({ where: { package_id: id, state: { in: ['queued', 'dispatched', 'building'] } } })
    if (active) throw new ORPCError('CONFLICT', { message: 'A build is already queued or running.' })
    await enqueue(tx, c, sign)
    await tx.review_event.create({ data: { id: randomUUID(), candidate_id: c.id, actor, decision: 'rebuild', comment: '', created_at: now() } })
    return { queued: true }
  })
}
export async function updateAccess(db: Database, actor: string, input: { id: string; can_view: boolean; can_rebuild: boolean; can_approve: boolean; disabled: boolean }) {
  if (input.id === actor && input.disabled) throw new ORPCError('BAD_REQUEST', { message: 'You cannot disable your own account.' })
  const { id, ...data } = input
  await transaction(db, async tx => {
    if (!await tx.web_user.findUnique({ where: { id } })) throw missing()
    await tx.web_user.update({ where: { id }, data })
    await tx.web_session.deleteMany({ where: { user_id: id } })
    await tx.access_audit.create({ data: { id: randomUUID(), actor, target: id, detail: `view=${data.can_view},rebuild=${data.can_rebuild},approve=${data.can_approve},disabled=${data.disabled}`, created_at: now() } })
  })
  return { updated: true }
}
