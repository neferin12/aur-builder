import { diffLines } from 'diff'
import type { Snapshot } from './payloads'
export function sourceDiffs(old: Snapshot, next: Snapshot) {
  const consumed = new Set<string>()
  type File = Snapshot['files'][number]
  function diff(before: File | undefined, after: File | undefined) {
    const binary = before?.content === null || after?.content === null
    return {
      path: (after ?? before)!.path, oldPath: before?.path,
      kind: !before ? 'added' : !after ? 'deleted' : before.path !== after.path ? 'renamed' : 'modified',
      message: binary ? `Binary, submodule, or oversized file. Old: ${before?.size ?? 0} bytes (${before?.hash ?? 'absent'}); new: ${after?.size ?? 0} bytes (${after?.hash ?? 'absent'}). Review outside the UI before approving.` : before && after && before.mode !== after.mode ? `File mode changed: ${before.mode.toString(8)} → ${after.mode.toString(8)}` : '',
      // Bound pathological diffs; show original text if the operation times out.
      lines: binary ? [] : (diffLines(before?.content ?? '', after?.content ?? '', { timeout: 500 }) ?? []).map(c => ({ kind: c.added ? 'insert' : c.removed ? 'delete' : 'equal', text: c.value })),
      oldText: binary ? null : before?.content ?? '', newText: binary ? null : after?.content ?? '',
    }
  }
  const result = []
  for (const file of next.files) {
    const previous = old.files.find(o => o.path === file.path) ?? old.files.find(o => o.hash === file.hash && !next.files.some(n => n.path === o.path) && !consumed.has(o.path))
    if (previous) consumed.add(previous.path)
    if (previous?.path === file.path && previous.hash === file.hash && previous.mode === file.mode) continue
    result.push(diff(previous, file))
  }
  for (const file of old.files) if (!consumed.has(file.path)) result.push(diff(file, undefined))
  return result
}
