import { defineConfig } from 'vitepress'
import { mermaidPlugin } from './plugins/mermaid.mjs'

const base = process.env.DOCS_BASE || '/'
if (!base.startsWith('/') || !base.endsWith('/')) {
  throw new Error('DOCS_BASE must start and end with / (for example /aur-builder/)')
}

export default defineConfig({
  title: 'AUR Builder',
  description: 'Build Arch packages. Publish a private, signed pacman repository.',
  lang: 'en-US',
  base,
  head: [['link', { rel: 'icon', type: 'image/svg+xml', href: `${base}logo.svg` }]],
  lastUpdated: true,
  markdown: { config: (md) => md.use(mermaidPlugin) },
  themeConfig: {
    logo: '/logo.svg',
    nav: [
      { text: 'Guide', link: '/guide/getting-started', activeMatch: '/guide/' },
      { text: 'Configuration', link: '/reference/configuration', activeMatch: '/reference/' },
      { text: 'v2', link: '/guide/upgrading' }
    ],
    sidebar: [
      {
        text: 'Start here',
        items: [
          { text: 'Introduction', link: '/guide/introduction' },
          { text: 'Getting started', link: '/guide/getting-started' },
          { text: 'Architecture', link: '/guide/architecture' }
        ]
      },
      {
        text: 'Deploy & use',
        items: [
          { text: 'Package & service configuration', link: '/reference/configuration' },
          { text: 'Garage storage', link: '/guide/storage' },
          { text: 'GPG signing', link: '/guide/signing' },
          { text: 'Release reviews & SSO', link: '/guide/reviews' },
          { text: 'Connect pacman clients', link: '/guide/pacman' },
          { text: 'Nginx reverse proxies', link: '/guide/nginx' }
        ]
      },
      {
        text: 'Maintain & develop',
        items: [
          { text: 'Operations & database reset', link: '/guide/operations' },
          { text: 'Upgrade to v2', link: '/guide/upgrading' },
          { text: 'Troubleshooting', link: '/guide/troubleshooting' },
          { text: 'Development & CI', link: '/guide/development' }
        ]
      }
    ],
    search: { provider: 'local' },
    socialLinks: [{ icon: 'github', link: 'https://github.com/neferin12/aur-builder' }],
    outline: [2, 3],
    footer: { message: 'AUR Builder · Self-hosted package infrastructure' }
  }
})
