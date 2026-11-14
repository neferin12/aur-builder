<script setup lang="ts">
import type { sourceDiffs } from '../../server/utils/diffs'
defineProps<{ diffs: ReturnType<typeof sourceDiffs> }>()
</script>
<template>
  <p v-if="!diffs.length" class="muted">No source file changes.</p>
  <section v-for="file in diffs" :key="file.path" class="panel">
    <h2>{{ file.path }} <Tag :value="file.kind" severity="secondary" /></h2>
    <p v-if="file.oldPath && file.oldPath !== file.path">Previously: {{ file.oldPath }}</p>
    <Message v-if="file.message" severity="warn">{{ file.message }}</Message>
    <pre v-if="file.lines.length"><span v-for="(line, index) in file.lines" :key="index" :class="['diff-line', line.kind]">{{ line.kind === 'insert' ? '+' : line.kind === 'delete' ? '−' : ' ' }} {{ line.text }}</span></pre>
    <template v-else-if="file.oldText !== null"><h3>Before</h3><pre>{{ file.oldText }}</pre><h3>After</h3><pre>{{ file.newText }}</pre></template>
  </section>
</template>
