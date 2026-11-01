import test from 'node:test'
import assert from 'node:assert/strict'
import { mermaidPlugin } from './mermaid.mjs'

test('Mermaid source round-trips without becoming Vue markup', () => {
  const source = 'flowchart LR\n  A["<img src=x onerror=alert(1)>"] --> B["é & {{ value }}"]\n'
  const md = { renderer: { rules: { fence: () => 'original fence' } } }
  mermaidPlugin(md)
  const html = md.renderer.rules.fence([{ info: 'mermaid', content: source }], 0)
  assert.equal(decodeURIComponent(html.match(/code="([^"]*)"/)[1]), source)
  assert.ok(!html.includes('<img'))
  assert.ok(!html.includes('{{ value }}'))
})

test('Other code fences retain VitePress rendering and arguments', () => {
  const tokens = [{ info: 'sh', content: 'echo hello' }]
  const md = { renderer: { rules: { fence: (...args) => {
    assert.deepEqual(args, [tokens, 0, 'options', 'env', 'renderer'])
    return 'highlighted shell'
  } } } }
  mermaidPlugin(md)
  assert.equal(md.renderer.rules.fence(tokens, 0, 'options', 'env', 'renderer'), 'highlighted shell')
})
