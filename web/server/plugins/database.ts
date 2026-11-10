import { assertSchema, database } from '../utils/db'
import { authConfig } from '../utils/config'
export default defineNitroPlugin(async nitro => {
  try {
    authConfig()
    const db = database()
    await assertSchema(db)
    nitro.hooks.hook('close', () => db.$disconnect())
  } catch (error) {
    console.error('Cannot start Nuxt: check database connectivity, configuration, and Rust migrations.', error instanceof Error ? error.message : 'Unknown initialization error')
    // Nitro reports rejected plugin promises but can otherwise keep listening.
    // Exit so Compose never treats an incompatible deployment as a running web app.
    process.exit(1)
  }
})
