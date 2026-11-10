<script setup lang="ts">
const { user, can } = useIdentity()
const { $rpc } = useNuxtApp()
const logoutError = ref('')
async function signOut() {
  try { await $rpc.logout(); await navigateTo('/auth/signed-out', { external: true }) }
  catch { logoutError.value = 'Sign out failed. Please try again.' }
}
</script>
<template>
  <div class="app-shell">
    <header class="app-header">
      <NuxtLink to="/" class="brand"><i class="pi pi-box" aria-hidden="true" /> AUR Builder</NuxtLink>
      <nav aria-label="Main navigation" v-if="user">
        <NuxtLink to="/">Packages</NuxtLink><NuxtLink to="/reviews">Reviews</NuxtLink><NuxtLink v-if="can('admin')" to="/admin/users">Users</NuxtLink>
      </nav>
      <div v-if="user" class="account"><span>{{ user.display_name }}</span><Button label="Sign out" icon="pi pi-sign-out" severity="secondary" size="small" @click="signOut" /></div>
    </header>
    <main id="main"><Message v-if="logoutError" severity="error">{{ logoutError }}</Message><NuxtPage /></main>
    <footer>AUR Builder · Package builds and release reviews</footer>
  </div>
</template>
