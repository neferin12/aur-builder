export default defineNuxtRouteMiddleware(async to => {
  if (to.path.startsWith('/auth/')) return
  const { $rpc } = useNuxtApp()
  try {
    const session = await $rpc.session()
    useState('identity', () => session.user).value = session.user
    useState('csrf', () => '').value = session.csrf
  } catch (error) {
    if (error && typeof error === 'object' && 'code' in error && error.code === 'UNAUTHORIZED') return navigateTo('/auth/login', { external: true })
    throw createError({ statusCode: 503, message: 'Unable to load your session' })
  }
})
