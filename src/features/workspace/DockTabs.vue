<script setup lang="ts">
const props = defineProps<{
  id: string;
  label: string;
  tabs: { id: string; label: string; badge?: number }[];
  modelValue: string;
}>();
const emit = defineEmits<{ 'update:modelValue': [id: string] }>();
function navigate(event: KeyboardEvent, index: number) {
  let next: number;
  if (event.key === 'ArrowRight') next = (index + 1) % props.tabs.length;
  else if (event.key === 'ArrowLeft') next = (index + props.tabs.length - 1) % props.tabs.length;
  else if (event.key === 'Home') next = 0;
  else if (event.key === 'End') next = props.tabs.length - 1;
  else return;
  const tab = props.tabs[next];
  if (!tab) return;
  event.preventDefault();
  emit('update:modelValue', tab.id);
  if (event.currentTarget instanceof HTMLElement)
    event.currentTarget.parentElement
      ?.querySelectorAll<HTMLButtonElement>('[role=tab]')
      [next]?.focus();
}
</script>

<template>
  <div class="dock-tabs" role="tablist" :aria-label="label">
    <button
      v-for="(tab, index) in tabs"
      :id="`${id}-tab-${tab.id}`"
      :key="tab.id"
      role="tab"
      :aria-selected="modelValue === tab.id"
      :aria-controls="`${id}-panel-${tab.id}`"
      :tabindex="modelValue === tab.id ? 0 : -1"
      @click="emit('update:modelValue', tab.id)"
      @keydown="navigate($event, index)"
    >
      {{ tab.label }}<span v-if="tab.badge !== undefined" class="tab-badge">{{ tab.badge }}</span>
    </button>
  </div>
</template>
