<script setup lang="ts">
defineProps<{
  name:
    | 'brand'
    | 'move'
    | 'region'
    | 'folder'
    | 'text'
    | 'image'
    | 'shape'
    | 'object'
    | 'adjustment'
    | 'unknown'
    | 'eye'
    | 'eyeOff'
    | 'lock';
}>();
const paths = {
  brand: 'm3 7 9-5 9 5-9 5-9-5Zm0 5 9 5 9-5M3 17l9 5 9-5',
  move: 'M12 2v20M2 12h20M9 5l3-3 3 3M9 19l3 3 3-3M5 9l-3 3 3 3M19 9l3 3-3 3',
  region: 'M8 3H3v5m13-5h5v5M3 16v5h5m8 0h5v-5M11 3h2M3 11v2m8 8h2m8-10v2',
  folder: 'M3 6h7l2 3h9v11H3V6Z',
  text: 'M4 7V4h16v3M12 4v16m-4 0h8',
  image: 'M3 3h18v18H3V3Zm0 14 6-6 5 5 3-3 4 4M16 7h.01',
  shape: 'm12 3 9 9-9 9-9-9 9-9Z',
  object: 'M4 3h12l4 4v14H4V3Zm12 0v5h4M8 12h8v5H8v-5Z',
  adjustment: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Zm0 0v18m0-14h6m-6 5h9m-9 5h6',
  unknown: 'M4 3h16v18H4V3Zm5 5a3 3 0 0 1 6 0c0 2-3 2-3 5m0 4h.01',
  eye: 'M2 12s3-7 10-7 10 7 10 7-3 7-10 7S2 12 2 12Zm10-3a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z',
  eyeOff: 'm3 3 18 18M9 5c6-2 11 3 13 7a19 19 0 0 1-4 5M6 6a19 19 0 0 0-4 6s3 7 10 7c1 0 2 0 3-.5',
  lock: 'M5 10h14v11H5V10Zm3 0V6a4 4 0 0 1 8 0v4m-4 5v2',
};
</script>

<template>
  <svg
    class="workspace-icon"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.6"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path :d="paths[name]" />
  </svg>
</template>
