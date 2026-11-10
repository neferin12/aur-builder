import type { web_user } from '../../generated/sqlite/index.js'
export function useIdentity() {
  const user = useState<web_user | null>('identity', () => null)
  const can = (permission: 'view' | 'rebuild' | 'approve' | 'admin') => !!user.value && !user.value.disabled && (user.value.admin || (user.value.can_view && (permission === 'view' || (permission === 'rebuild' && user.value.can_rebuild) || (permission === 'approve' && user.value.can_approve))))
  return { user, can }
}
