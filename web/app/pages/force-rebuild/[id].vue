<script setup lang="ts">
await navigateTo(`/build-results/${useRoute().params.id}`, { replace: true, redirectCode: 302 })
</script>
<template><p>Opening build history…</p></template>
