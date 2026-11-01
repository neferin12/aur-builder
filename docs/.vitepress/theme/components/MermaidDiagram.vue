<script setup>
import { computed, onBeforeUnmount, onMounted, ref, useId, watch } from 'vue'
import { useData } from 'vitepress'

const props = defineProps({ code: { type: String, required: true } })
const { isDark } = useData()
const source = computed(() => decodeURIComponent(props.code))
const svg = ref('')
const failed = ref(false)
const id = `mermaid-${useId().replace(/[^a-zA-Z0-9_-]/g, '-')}`
let revision = 0

onMounted(() => {
  watch([source, isDark], async () => {
    const current = ++revision
    failed.value = false
    try {
      // Mermaid needs the DOM: never import it while prerendering the site.
      const { default: mermaid } = await import('mermaid')
      if (current !== revision) return
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: 'strict',
        theme: isDark.value ? 'dark' : 'base',
        themeVariables: isDark.value ? {} : {
          primaryColor: '#eaf6ef', primaryTextColor: '#182c3b',
          primaryBorderColor: '#82b4a1', lineColor: '#647b70',
          secondaryColor: '#f1f6f4', tertiaryColor: '#ffffff'
        },
        layout: 'dagre',
        fontFamily: 'system-ui, sans-serif',
        flowchart: { htmlLabels: false }
      })
      const result = await mermaid.render(`${id}-${current}`, source.value)
      if (current === revision) svg.value = result.svg
    } catch {
      if (current === revision) {
        svg.value = ''
        failed.value = true
      }
    }
  }, { immediate: true })
})

onBeforeUnmount(() => { revision++ })
</script>

<template>
  <figure class="mermaid-diagram">
    <div v-if="svg" class="mermaid-canvas" tabindex="0" aria-label="Scrollable diagram" v-html="svg" />
    <pre v-else><code>{{ source }}</code></pre>
    <p v-if="failed" role="alert">Diagram rendering failed. The Mermaid source is shown above.</p>
    <figcaption v-if="svg">Scroll horizontally on smaller screens to see the full diagram.</figcaption>
  </figure>
</template>

<style scoped>
.mermaid-diagram { margin: 24px 0; }
.mermaid-canvas { overflow-x: auto; padding: 16px; border: 1px solid var(--vp-c-divider); border-radius: 8px; }
.mermaid-canvas :deep(svg) { display: block; min-width: 640px; width: 100%; max-width: none !important; height: auto; }
.mermaid-canvas:focus-visible { outline: 2px solid var(--vp-c-brand-1); outline-offset: 3px; }
figcaption { margin-top: 8px; color: var(--vp-c-text-2); font-size: 12px; }
pre { overflow-x: auto; padding: 16px; background: var(--vp-code-block-bg); border-radius: 8px; font-size: 12px; }
</style>
