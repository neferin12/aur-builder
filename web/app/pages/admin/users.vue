<script setup lang="ts">
const { $rpc } = useNuxtApp()
const { user: currentUser } = useIdentity()
const { data, error, refresh } = await useAsyncData('users', () => $rpc.users())
const actionError = ref<unknown>(), busy = ref('')
async function save(user: NonNullable<typeof data.value>[number]) {
  busy.value = user.id
  try { await $rpc.updateUser({ id: user.id, can_view: user.can_view, can_rebuild: user.can_rebuild, can_approve: user.can_approve, disabled: user.disabled }); if (user.id === currentUser.value?.id) { await navigateTo('/auth/login', { external: true }); return }; await refresh() } catch(e) { actionError.value = e } finally { busy.value = '' }
}
</script>
<template><h1>User access</h1><p class="subtitle">Administrators are assigned by the identity provider. Saving access revokes the user’s sessions.</p><RequestError :error="error ?? actionError"/><DataTable :value="data ?? []"><template #empty>No users.</template><Column field="display_name" header="User"/><Column header="Administrator"><template #body="{ data: row }"><Tag :value="row.admin ? 'Yes' : 'No'"/></template></Column><Column v-for="grant in ['can_view', 'can_rebuild', 'can_approve', 'disabled']" :key="grant" :header="grant.replace('can_', '')"><template #body="{ data: row }"><Checkbox v-model="row[grant]" binary :aria-label="`${row.display_name}: ${grant}`"/></template></Column><Column header="Actions"><template #body="{ data: row }"><Button label="Save" size="small" :loading="busy === row.id" @click="save(row)"/></template></Column></DataTable></template>
