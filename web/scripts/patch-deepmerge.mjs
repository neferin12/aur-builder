// GHSA-gcqx-w48m-w2h4 has no upstream release yet. Patch the exact locked
// build dependency; fail closed if its distribution changes. Remove this when
// Rollup no longer needs deepmerge or an upstream fixed release is available.
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'

for (const [file, digest] of Object.entries({
  'cjs.js': '2e5160f84a1141bd927264afe4d15766f1e6cf07daef182b37ff53fea121de7d',
  'umd.js': 'f33e6596482d1fb5c7c427f8ef92f2aa849eea9223acd974176b2fb27ddbf73a',
})) {
  const path = new URL(`../node_modules/deepmerge/dist/${file}`, import.meta.url)
  const original = readFileSync(path, 'utf8')
  const guard = "if (key === '__proto__' || key === 'constructor' || key === 'prototype') return; // aur-builder security patch"
  if (original.split(guard).length === 3) continue
  if (createHash('sha256').update(original).digest('hex') !== digest) throw new Error(`Unexpected deepmerge distribution: ${file}`)
  let replacements = 0
  const patched = original.replace(/getKeys\((target|source)\)\.forEach\(function\(key\) \{/g, match => {
    replacements++
    return `${match}\n            ${guard}`
  })
  if (replacements !== 2) throw new Error(`Unexpected deepmerge merge implementation: ${file}`)
  writeFileSync(path, patched)
}
