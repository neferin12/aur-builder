<script setup lang="ts">
defineProps<{ status: string }>()
</script>
<template><Tag :value="status" :severity="status === 'success' || status === 'approved' ? 'success' : status === 'failed' || status === 'rejected' ? 'danger' : 'secondary'" /></template>
