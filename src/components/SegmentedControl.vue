<script setup lang="ts" generic="T extends string">
defineProps<{ options: readonly { value: T; label: string }[]; label: string }>();
const model = defineModel<T>({ required: true });
</script>

<template>
  <div class="segmented-control" role="radiogroup" :aria-label="label">
    <button
      v-for="option in options"
      :key="option.value"
      type="button"
      role="radio"
      :aria-checked="model === option.value"
      @click="model = option.value"
    >{{ option.label }}</button>
  </div>
</template>

<style scoped>
.segmented-control {
  display: inline-flex;
  flex-wrap: wrap;
  max-width: 100%;
  flex-shrink: 0;
  gap: var(--space-1);
  padding: var(--space-1);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-pill);
}
.segmented-control button {
  flex: 1 1 auto;
  min-width: 0;
  min-height: 40px;
  padding: var(--space-2) var(--space-3);
  overflow-wrap: anywhere;
  border: 0;
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--color-text-muted);
  font-size: var(--fs-control);
  font-weight: 600;
  cursor: pointer;
}
.segmented-control button:hover { color: var(--color-text); }
.segmented-control button[aria-checked="true"] { background: var(--color-accent); color: var(--color-on-accent); }
.segmented-control button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
</style>
