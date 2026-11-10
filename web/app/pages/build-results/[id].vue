<script setup lang="ts">
const { $rpc } = useNuxtApp(), route = useRoute(), { can } = useIdentity()
const id = Number(route.params.id)
const { data, error } = await useAsyncData(`history-${id}`, () => $rpc.history({ id }))
const actionError = ref<unknown>(), queued = ref(false), busy = ref(false)
async function rebuild() {
  busy.value = true
  try { await $rpc.rebuild({ id }); queued.value = true } catch (e) { actionError.value = e } finally { busy.value = false }
}
</script>
<template>
  <NuxtLink to="/">← Packages</NuxtLink><RequestError :error="error"/>
  <template v-if="data"><div class="toolbar"><div><h1>{{ data.package.name }}</h1><p class="muted">{{ data.package.version }} · {{ data.package.maintainer }}</p></div><Button v-if="can('rebuild')" label="Rebuild approved revision" icon="pi pi-replay" :disabled="queued" :loading="busy" @click="rebuild"/></div>
  <RequestError :error="actionError"/><Message v-if="queued" severity="success">Build queued for the approved revision.</Message>
  <DataTable :value="data.results" paginator :rows="25"><template #empty>No builds recorded.</template>
    <Column header="Build"><template #body="{ data: row }"><NuxtLink :to="`/build-log/${row.id}`">#{{ row.id }}</NuxtLink></template></Column>
    <Column field="version" header="Version"/><Column header="Status"><template #body="{ data: row }"><BuildStatus :status="row.success ? 'success' : 'failed'"/></template></Column>
    <Column field="started_at" header="Started"/><Column field="finished_at" header="Finished"/><Column field="exit_code" header="Exit code"/>
  </DataTable></template>
</template>
