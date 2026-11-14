import { PrismaBetterSQLite3 } from '@prisma/adapter-better-sqlite3'
import { PrismaPg } from '@prisma/adapter-pg'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { resolve, dirname } from 'node:path'
import { existsSync } from 'node:fs'
import type { Prisma as SQLite } from '../../generated/sqlite/index.js'
import type { Prisma as Postgres } from '../../generated/postgresql/index.js'
import { required } from './config'
// Keep Prisma's generated CommonJS client intact rather than rebundling
// them into Nitro ESM. Both locations are deterministic in source and production.
const require = createRequire(import.meta.url)
function clientPath(provider: string) {
  const source = fileURLToPath(new URL(`../../generated/${provider}/index.js`, import.meta.url))
  return existsSync(source) ? source : resolve(dirname(process.argv[1]!), `generated/${provider}/index.js`)
}
const SQLiteClient: typeof import('../../generated/sqlite/index.js').PrismaClient = require(clientPath('sqlite')).PrismaClient
const PostgresClient: typeof import('../../generated/postgresql/index.js').PrismaClient = require(clientPath('postgresql')).PrismaClient
export function databaseUrl(value: string) {
  if (value.startsWith('sqlite:') || value.startsWith('file:')) {
    const path = value.replace(/^(sqlite:|file:)/, '').split('?')[0]!
    return `file:${resolve(path)}`
  }
  return value.replace(/^postgres:/, 'postgresql:')
}
// The adapter exposes full schema-derived rows, avoiding incompatible generic
// delegate overloads between Prisma's two generated provider clients.
function adapt(sqlite: SQLite.TransactionClient, postgres: Postgres.TransactionClient, isPostgres: boolean) {
 return {
  web_user: {
   findUnique: (args: Omit<SQLite.web_userFindUniqueArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.web_user.findUnique(args)
    return sqlite.web_user.findUnique(args)
   },
   findMany: (args?: Omit<SQLite.web_userFindManyArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.web_user.findMany(args)
    return sqlite.web_user.findMany(args)
   },
   update: (args: Omit<SQLite.web_userUpdateArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.web_user.update(args)
    return sqlite.web_user.update(args)
   },
   upsert: (args: Omit<SQLite.web_userUpsertArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.web_user.upsert(args)
    return sqlite.web_user.upsert(args)
   },
  },
  web_session: {
   findFirst: (args?: Omit<SQLite.web_sessionFindFirstArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.web_session.findFirst(args)
    return sqlite.web_session.findFirst(args)
   },
   deleteMany: (args?: Omit<SQLite.web_sessionDeleteManyArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.web_session.deleteMany(args)
    return sqlite.web_session.deleteMany(args)
   },
   create: (args: Omit<SQLite.web_sessionCreateArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.web_session.create(args)
    return sqlite.web_session.create(args)
   },
  },
  oidc_login: {
   findFirst: (args?: Omit<SQLite.oidc_loginFindFirstArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.oidc_login.findFirst(args)
    return sqlite.oidc_login.findFirst(args)
   },
   deleteMany: (args?: Omit<SQLite.oidc_loginDeleteManyArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.oidc_login.deleteMany(args)
    return sqlite.oidc_login.deleteMany(args)
   },
   create: (args: Omit<SQLite.oidc_loginCreateArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.oidc_login.create(args)
    return sqlite.oidc_login.create(args)
   },
  },
  seaql_migrations: {
   findMany: (args?: Omit<SQLite.seaql_migrationsFindManyArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.seaql_migrations.findMany(args)
    return sqlite.seaql_migrations.findMany(args)
   },
  },
  package_metadata: {
   findMany: (args?: Omit<SQLite.package_metadataFindManyArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.package_metadata.findMany(args)
    return sqlite.package_metadata.findMany(args)
   },
   findUnique: (args: Omit<SQLite.package_metadataFindUniqueArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.package_metadata.findUnique(args)
    return sqlite.package_metadata.findUnique(args)
   },
   update: (args: Omit<SQLite.package_metadataUpdateArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.package_metadata.update(args)
    return sqlite.package_metadata.update(args)
   },
  },
  build_attempt: {
   findMany: (args?: Omit<SQLite.build_attemptFindManyArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.build_attempt.findMany(args)
    return sqlite.build_attempt.findMany(args)
   },
   findFirst: (args?: Omit<SQLite.build_attemptFindFirstArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.build_attempt.findFirst(args)
    return sqlite.build_attempt.findFirst(args)
   },
   create: (args: Omit<SQLite.build_attemptCreateArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.build_attempt.create(args)
    return sqlite.build_attempt.create(args)
   },
  },
  release_candidate: {
   findMany: (args?: Omit<SQLite.release_candidateFindManyArgs, 'select' | 'include' | 'omit'>) => {
    const select = { id: true, payload: true, revision: true, state: true, created_at: true } as const
    if (isPostgres) return postgres.release_candidate.findMany({ ...args, select })
    return sqlite.release_candidate.findMany({ ...args, select })
   },
   findUnique: (args: Omit<SQLite.release_candidateFindUniqueArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.release_candidate.findUnique(args)
    return sqlite.release_candidate.findUnique(args)
   },
   findFirst: (args?: Omit<SQLite.release_candidateFindFirstArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.release_candidate.findFirst(args)
    return sqlite.release_candidate.findFirst(args)
   },
   updateMany: (args: Omit<SQLite.release_candidateUpdateManyArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.release_candidate.updateMany(args)
    return sqlite.release_candidate.updateMany(args)
   },
  },
  review_event: {
   findMany: (args?: Omit<SQLite.review_eventFindManyArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.review_event.findMany(args)
    return sqlite.review_event.findMany(args)
   },
   create: (args: Omit<SQLite.review_eventCreateArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.review_event.create(args)
    return sqlite.review_event.create(args)
   },
  },
  ai_review: {
   findUnique: (args: Omit<SQLite.ai_reviewFindUniqueArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.ai_review.findUnique(args)
    return sqlite.ai_review.findUnique(args)
   },
  },
  access_audit: {
   findMany: (args?: Omit<SQLite.access_auditFindManyArgs, 'select' | 'include' | 'omit'>) => isPostgres ? postgres.access_audit.findMany(args) : sqlite.access_audit.findMany(args),
   create: (args: Omit<SQLite.access_auditCreateArgs, 'select' | 'include' | 'omit'>) => {
    if (isPostgres) return postgres.access_audit.create(args)
    return sqlite.access_audit.create(args)
   },
  },
  build_results: {
   latestStatuses: async () => {
     const query = `SELECT b.package_id, b.success FROM build_results b JOIN (SELECT package_id, MAX(id) AS id FROM build_results GROUP BY package_id) latest ON b.id = latest.id`
     return isPostgres
       ? postgres.$queryRawUnsafe<{ package_id: bigint; success: boolean }[]>(query)
       : sqlite.$queryRawUnsafe<{ package_id: bigint; success: boolean }[]>(query)
   },
   findMany: (args?: { take?: number; where?: Pick<SQLite.build_resultsWhereInput, 'package_id' | 'id'>; orderBy?: Pick<SQLite.build_resultsOrderByWithRelationInput, 'id'> }) => {
     const select = { id: true, package_id: true, exit_code: true, success: true, version: true, started_at: true, finished_at: true } as const
     return isPostgres ? postgres.build_results.findMany({ ...args, select }) : sqlite.build_results.findMany({ ...args, select })
   },
   findUnique: (args: { where: { id: NonNullable<SQLite.build_resultsWhereUniqueInput['id']> } }) => isPostgres ? postgres.build_results.findUnique(args) : sqlite.build_results.findUnique(args),
  },
 }
}
export function createSQLiteClient(url: string) {
  return new SQLiteClient({ adapter: new PrismaBetterSQLite3({ url }) })
}
export function createPostgresClient(url: string) {
  return new PostgresClient({ adapter: new PrismaPg({ connectionString: url }) })
}
export function createDatabase(url: string) {
  const normalized = databaseUrl(url), isPostgres = !normalized.startsWith('file:')
  // Only the selected client connects; the other supplies its generated type contract.
  const sqlite = (isPostgres ? undefined : createSQLiteClient(normalized))!
  const postgres = (isPostgres ? createPostgresClient(normalized) : undefined)!
  return {
    ...adapt(sqlite, postgres, isPostgres),
    transaction: <T>(work: (tx: Transaction) => Promise<T>) => isPostgres
      ? postgres.$transaction(tx => work(adapt(sqlite, tx, true)), { timeout: 15000 })
      : sqlite.$transaction(tx => work(adapt(tx, postgres, false)), { timeout: 15000 }),
    $disconnect: async () => { await Promise.all([sqlite?.$disconnect(), postgres?.$disconnect()]) },
  }
}
export type Database = ReturnType<typeof createDatabase>
export type Transaction = ReturnType<typeof adapt>
let instance: Database | undefined
export function database() { return instance ??= createDatabase(required('DATABASE_URL')) }
export async function transaction<T>(db: Database, work: (tx: Transaction) => Promise<T>): Promise<T> {
  for (let attempt = 0; ; attempt++) {
    try { return await db.transaction(work) }
    catch (error) {
      if (attempt >= 3 || !(error instanceof Error) || !('code' in error) || !['P2034', 'P1008', 'P2028'].includes(String(error.code))) throw error
    }
  }
}
export async function assertSchema(db: Database) {
  const versions = await db.seaql_migrations.findMany()
  if (!versions.some(v => v.version === 'm20261111_000001_security_indexes')) throw new Error('Database schema is outdated; run Rust migrations before starting Nuxt')
}
