<script setup lang="ts">
defineProps<{ error: unknown }>()
</script>
<template><Message v-if="error" severity="error" role="alert">{{ error instanceof Error ? error.message : 'The request failed. Please try again.' }}</Message></template>
