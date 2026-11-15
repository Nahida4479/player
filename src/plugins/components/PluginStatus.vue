<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, useId, watch } from 'vue';
import ResponsivePopup from '../../components/ResponsivePopup.vue';
import PluginImage from './PluginImage.vue';
import type { PluginManifest, UiNode } from '../types';
const props = defineProps<{ node: UiNode; manifest: PluginManifest }>();
const open = ref(false);
const root = ref<HTMLElement>();
const trigger = ref<HTMLButtonElement>();
const id = useId();
watch(open, async value => {
  if (!value) return;
  await nextTick();
  const menus = document.querySelectorAll<HTMLElement>(`[data-plugin-status-menu="${id}"]`);
  for (const menu of menus) if (menu.getBoundingClientRect().width) { menu.querySelector<HTMLButtonElement>('button')?.focus(); break; }
});
async function close() {
  if (!open.value) return;
  open.value = false;
  await nextTick();
  trigger.value?.focus();
}
function outside(event: PointerEvent) {
  if (open.value && !root.value?.contains(event.target as Node) && !(event.target as Element).closest?.(`[data-plugin-status-menu="${id}"]`)) void close();
}
function keydown(event: KeyboardEvent) {
  if (open.value && event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); void close(); }
}
watch(() => props.node.children?.length, length => { if (!length) void close(); });
onMounted(() => { document.addEventListener('pointerdown', outside); window.addEventListener('keydown', keydown, true); });
onUnmounted(() => { document.removeEventListener('pointerdown', outside); window.removeEventListener('keydown', keydown, true); });
</script>
<template>
  <div ref="root" class="plugin-status" :class="`plugin-status-${node.tone}`">
    <ResponsivePopup v-if="node.children?.length" :open="open" panel-class="plugin-status-popup" @close="close">
      <template #trigger><button ref="trigger" class="plugin-status-trigger" :aria-label="node.label" :title="node.label" aria-haspopup="dialog" :aria-expanded="open" @click.stop="open ? close() : open = true"><PluginImage :manifest="manifest" monochrome tint="currentColor" /></button></template>
      <section :data-plugin-status-menu="id" role="dialog" :aria-label="node.title || manifest.name" class="plugin-status-content">
        <header><h3>{{ node.title || manifest.name }}</h3><button class="control-button" aria-label="Close plugin menu" @click="close">Close</button></header>
        <slot />
      </section>
    </ResponsivePopup>
    <span v-else role="status" :aria-label="node.label" :title="node.label" class="plugin-status-static"><PluginImage :manifest="manifest" monochrome tint="currentColor" /></span>
  </div>
</template>
<style scoped>
.plugin-status { display: flex; align-items: center; justify-content: center; align-self: center; color: var(--color-accent-text); --plugin-icon-size: 19px; }
.plugin-status-trigger,.plugin-status-static { display: flex; align-items: center; justify-content: center; width: 32px; height: 32px; padding: 0; border: 0; background: transparent; color: inherit; }
.plugin-status-trigger { cursor: pointer; border-radius: 4px; }
.plugin-status-trigger:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
.plugin-status-active .plugin-status-trigger :deep(.plugin-image),.plugin-status-active .plugin-status-static :deep(.plugin-image) { animation: plugin-status-pulse 1.4s ease-in-out infinite; }
.plugin-status-error { color: var(--color-danger-text); }
.plugin-status-success { color: #39b86b; }
.plugin-status-content { padding: var(--space-4); color: var(--color-text); overflow-wrap: anywhere; }
header { display: flex; align-items: center; justify-content: space-between; gap: var(--space-4); margin-bottom: var(--space-4); }
h3 { margin: 0; }
@keyframes plugin-status-pulse { 50% { opacity: .35; } }
@media(prefers-reduced-motion: reduce) { .plugin-status-active .plugin-status-trigger :deep(.plugin-image),.plugin-status-active .plugin-status-static :deep(.plugin-image) { animation: none; } }
</style>
<style>
.responsive-popup-desktop.plugin-status-popup { width: min(380px, calc(100vw - 24px)); max-width: calc(100vw - 24px); }
</style>
