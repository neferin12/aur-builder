<script setup lang="ts">
const { $rpc } = useNuxtApp(), { can } = useIdentity()
const { data, error, refresh, status } = await useAsyncData('packages', () => $rpc.packages())
const search = ref('')
const rows = computed(() => data.value?.packages.filter(p => p.name.toLowerCase().includes(search.value.toLowerCase())) ?? [])
const counts = computed(() => ['success', 'failed', 'unbuilt'].map(s => ({ status: s, count: data.value?.packages.filter(p => p.build_status === s).length ?? 0 })))
</script>
<template>
  <h1>Packages</h1><p class="subtitle">Build status and activity across your repository.</p>
  <Message v-if="!can('view')" severity="warn">Your account has no viewing permission. Ask an administrator for access.</Message>
  <RequestError :error="error" />
  <div class="stats"><Card v-for="item in counts" :key="item.status" class="stat"><template #content><strong>{{ item.count }}</strong>{{ item.status }}</template></Card></div>
  <section v-if="data?.activity.length" class="panel"><h2>Build activity</h2><DataTable :value="data.activity"><Column field="name" header="Package"/><Column field="version" header="Version"/><Column field="state" header="State"/></DataTable></section>
  <div class="toolbar"><InputText v-model="search" placeholder="Search packages" aria-label="Search packages"/><Button label="Refresh" icon="pi pi-refresh" severity="secondary" :loading="status === 'pending'" @click="refresh()"/></div>
  <DataTable :value="rows" paginator :rows="25" :loading="status === 'pending'" striped-rows>
    <template #empty>No packages tracked yet.</template>
    <Column field="name" header="Package" sortable><template #body="{ data: row }"><NuxtLink :to="`/build-results/${row.id}`">{{ row.name }}</NuxtLink></template></Column>
    <Column field="version" header="Version" sortable/><Column field="maintainer" header="Maintainer" sortable/>
    <Column field="build_status" header="Latest build" sortable><template #body="{ data: row }"><BuildStatus :status="row.build_status"/></template></Column>
  </DataTable>
</template>
