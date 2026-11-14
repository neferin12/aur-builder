<script setup lang="ts">
const { $rpc } = useNuxtApp(), id = String(useRoute().params.id), { can } = useIdentity()
const { data, error, refresh } = await useAsyncData(`review-${id}`, () => $rpc.review({ id }))
const comment = ref(''), busy = ref(false), actionError = ref<unknown>()
async function decide(decision: 'approve' | 'reject') {
  busy.value = true; actionError.value = undefined
  try { await $rpc.decide({ id, decision, comment: comment.value }); await refresh() } catch(e) { actionError.value = e } finally { busy.value = false }
}
</script>
<template><NuxtLink to="/reviews">← Reviews</NuxtLink><RequestError :error="error"/>
<template v-if="data"><h1>{{ data.candidate.name }} <BuildStatus :status="data.candidate.state"/></h1><div class="meta"><span>{{ data.candidate.version }}</span><code>{{ data.candidate.revision }}</code><span v-if="data.baseline_revision">Baseline: {{ data.baseline_revision }}</span></div>
<section class="panel"><h2>Build settings</h2><p>Source: {{ data.source ?? 'AUR' }}</p><p>Subfolder: {{ data.subfolder ?? 'Repository root' }}</p><p>Options: {{ data.options ?? 'Default' }}</p></section>
<Message v-if="data.incomplete" severity="warn">Source snapshot is incomplete. Review omitted files outside the UI before approving.</Message><Message v-if="data.settings_changed" severity="warn">Build settings or environment changed. Environment values are hidden.</Message>
<SourceDiff :diffs="data.diffs"/>
<section v-if="data.ai" class="panel"><h2>AI assessment</h2><p>Automated advice may be incorrect or influenced by upstream content. Automatic approval is an explicit supply-chain trust decision.</p><pre>{{ JSON.stringify(data.ai, null, 2) }}</pre></section>
<section v-if="can('approve') && data.candidate.state === 'pending'" class="panel review-form"><h2>Review decision</h2><label for="comment">Comment (required for rejection)</label><Textarea id="comment" v-model="comment" rows="4" :maxlength="16000"/><RequestError :error="actionError"/><div class="form-row"><Button label="Approve and queue build" icon="pi pi-check" :loading="busy" @click="decide('approve')"/><Button label="Reject" severity="danger" :disabled="!comment.trim()" :loading="busy" @click="decide('reject')"/></div></section>
<section class="panel"><h2>Review history</h2><p v-if="!data.events.length" class="muted">No decisions recorded.</p><article v-for="(event, index) in data.events" :key="index"><strong>{{ event.decision }}</strong> · {{ event.actor }} · {{ new Date(event.created_at * 1000).toISOString() }}<pre>{{ event.comment }}</pre></article></section>
</template></template>
