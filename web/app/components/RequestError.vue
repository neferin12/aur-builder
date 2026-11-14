<script setup lang="ts">
defineProps<{ error: unknown }>()
</script>
<template><Message v-if="error" severity="error" role="alert">{{ 'The request failed. Please try again.' }}</Message></template>
