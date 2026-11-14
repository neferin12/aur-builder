import { cpSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
export default defineNuxtConfig({
  compatibilityDate: '2025-11-01',
  devtools: { enabled: false },
  modules: ['@primevue/nuxt-module'],
  css: ['primeicons/primeicons.css', '~/assets/main.css'],
  primevue: { components: { include: ['Button', 'Card', 'Checkbox', 'Column', 'DataTable', 'InputText', 'Message', 'Select', 'Tag', 'Textarea'] }, importTheme: { from: fileURLToPath(new URL('./theme.ts', import.meta.url)) } },
  nitro: {
    preset: 'node-server', esbuild: { options: { target: 'es2022' } },
    hooks: {
      compiled(nitro) {
        cpSync(fileURLToPath(new URL('./generated', import.meta.url)), join(nitro.options.output.serverDir, 'generated'), { recursive: true })
      },
    },
  },
  typescript: { strict: true },
  app: { head: { title: 'AUR Builder', meta: [{ name: 'description', content: 'Package builds and release reviews' }] } },
})
