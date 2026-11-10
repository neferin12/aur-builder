// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import BuildLog from '../app/components/BuildLog.vue'
import SourceDiff from '../app/components/SourceDiff.vue'
import { sourceDiffs } from '../server/utils/diffs'
import { allows } from '../server/utils/permissions'
import type { web_user } from '../generated/sqlite/index.js'
const file = (path: string, hash: string, content: string | null, mode = 33188) => ({ path, hash, content, mode, size: 1 })
describe('safe rendering and review fidelity', () => {
  it('renders logs as text, never executable markup', () => {
    const wrapper = mount(BuildLog, { props: { log: '<script>alert(1)</script><img src=x onerror=alert(1)>' } })
    expect(wrapper.find('script').exists()).toBe(false)
    expect(wrapper.find('img').exists()).toBe(false)
    expect(wrapper.text()).toContain('<script>')
    expect(mount(BuildLog, { props: { log: null } }).text()).toContain('No log output')
  })
  it('handles renames, mode changes, binary omissions and additions/deletions', () => {
    const diffs = sourceDiffs({ complete: true, files: [file('old', 'same', 'safe'), file('removed', 'gone', 'gone'), file('mode', 'mode', 'same'), file('binary', 'old', null)] }, { complete: false, files: [file('renamed', 'same', 'safe'), file('added', 'new', '<script>bad</script>'), file('mode', 'mode', 'same', 33261), file('binary', 'new', null)] })
    expect(diffs.map(d => d.kind)).toEqual(['renamed', 'added', 'modified', 'modified', 'deleted'])
    expect(diffs.find(d => d.path === 'mode')?.message).toContain('mode changed')
    expect(diffs.find(d => d.path === 'binary')?.message).toContain('Review outside')
    const wrapper = mount(SourceDiff, { props: { diffs }, global: { stubs: { Tag: true, Message: { template: '<div><slot/></div>' } } } })
    expect(wrapper.find('script').exists()).toBe(false)
    expect(wrapper.text()).toContain('<script>bad</script>')
  })
  it('checks every permission combination including disabled administrators', () => {
    for (const admin of [false, true]) for (const disabled of [false, true]) for (const view of [false, true]) for (const rebuild of [false, true]) for (const approve of [false, true]) {
      const user: web_user = { id: 'u', issuer: 'i', subject: 's', display_name: 'U', admin, disabled, can_view: view, can_rebuild: rebuild, can_approve: approve }
      expect(allows(user, 'view')).toBe(!disabled && (admin || view))
      expect(allows(user, 'rebuild')).toBe(!disabled && (admin || (view && rebuild)))
      expect(allows(user, 'approve')).toBe(!disabled && (admin || (view && approve)))
      expect(allows(user, 'admin')).toBe(!disabled && admin)
      expect(allows(user, 'session')).toBe(!disabled)
    }
  })
})
