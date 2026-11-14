import type { web_user } from '../../generated/sqlite/index.js'
export type Permission = 'view' | 'rebuild' | 'approve' | 'admin' | 'session'
export function allows(user: web_user, permission: Permission) {
  return !user.disabled && (permission === 'session' || user.admin || (user.can_view && (permission === 'view' || (permission === 'rebuild' && user.can_rebuild) || (permission === 'approve' && user.can_approve))))
}
