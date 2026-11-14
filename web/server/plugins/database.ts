import { validateTrustedProxies } from '../utils/rate-limit'
import { assertSchema, database } from '../utils/db'
import { authConfig } from '../utils/config'
export default defineNitroPlugin(async nitro => {
  try {
    authConfig()
    validateTrustedProxies()
    const db = database()
    await assertSchema(db)
    const cleanup = setInterval(() => {
      void db.oidc_login.deleteMany({ where: { expires_at: { lte: BigInt(Math.floor(Date.now() / 1000)) } } }).catch(() => console.error('OIDC state cleanup failed'))
    }, 60000)
    cleanup.unref()
    nitro.hooks.hook('close', () => { clearInterval(cleanup); return db.$disconnect() })
  } catch (error) {
    console.error('Cannot start Nuxt: check database connectivity, configuration, and Rust migrations.', error instanceof Error ? error.message : 'Unknown initialization error')
    // Nitro reports rejected plugin promises but can otherwise keep listening.
    // Exit so Compose never treats an incompatible deployment as a running web app.
    process.exit(1)
  }
})
