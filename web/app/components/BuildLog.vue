<script setup lang="ts">
defineProps<{ log: string | null }>()
</script>
<template><pre class="log" aria-label="Build log">{{ log || 'No log output recorded.' }}</pre></template>
