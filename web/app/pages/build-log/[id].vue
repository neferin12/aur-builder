<script setup lang="ts">
import descriptions from '~/assets/error-codes.json'
const { $rpc } = useNuxtApp(), id = Number(useRoute().params.id)
const { data, error } = await useAsyncData(`log-${id}`, () => $rpc.buildLog({ id }))
const description = computed(() => data.value ? (descriptions as Record<string, string>)[data.value.result.exit_code] ?? 'Unknown error' : '')
</script>
<template><RequestError :error="error"/><template v-if="data"><NuxtLink :to="`/build-results/${data.package.id}`">← Build history</NuxtLink><h1>{{ data.package.name }} · Build #{{ data.result.id }}</h1><div class="meta"><BuildStatus :status="data.result.success ? 'success' : 'failed'"/><span>{{ data.result.version }}</span><span>Exit {{ data.result.exit_code }}: {{ description }}</span></div><BuildLog :log="data.result.build_log"/></template></template>
