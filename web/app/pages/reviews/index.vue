<script setup lang="ts">
const { $rpc } = useNuxtApp(), route = useRoute()
const statuses = ['pending', 'approved', 'rejected', 'superseded', 'all'] as const
const selected = computed({ get: () => statuses.find(s => s === route.query.status) ?? 'pending', set: value => { navigateTo({ query: { status: value } }) } })
const before = ref<string>()
watch(selected, () => { before.value = undefined })
const { data, error, status } = await useAsyncData('reviews', () => $rpc.reviews({ status: selected.value, before: before.value }), { watch: [selected, before] })
</script>
<template><h1>Release reviews</h1><p class="subtitle">Inspect packaging changes before approving a build.</p><div class="toolbar"><Select v-model="selected" :options="[...statuses]" aria-label="Review status"/></div><RequestError :error="error"/>
<DataTable :value="data ?? []" :loading="status === 'pending'" paginator :rows="25"><template #empty>No releases match this filter.</template><Column header="Package"><template #body="{ data: row }"><NuxtLink :to="`/reviews/${row.id}`">{{ row.name }}</NuxtLink></template></Column><Column field="version" header="Version"/><Column field="revision" header="Revision"/><Column header="State"><template #body="{ data: row }"><BuildStatus :status="row.state"/></template></Column></DataTable><Button v-if="before" label="Newest reviews" @click="before = undefined"/><Button v-if="data?.length === 100" label="More reviews" @click="before = data[data.length - 1]!.id"/></template>
