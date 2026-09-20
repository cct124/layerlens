<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref } from 'vue';
import type { WorkspaceMenu, WorkspaceMenuCommand, WorkspaceMenuItem } from './workspaceMenu';
import { workspaceShortcut } from './workspaceShortcuts';

const props = defineProps<{ menus: WorkspaceMenu[] }>();
const emit = defineEmits<{ command: [command: WorkspaceMenuCommand] }>();
const root = ref<HTMLElement | null>(null);
const opened = ref<string | null>(null);
const activeIndex = ref(0);
let previousFocus: HTMLElement | null = null;

function trigger(index: number) {
  return root.value?.querySelectorAll<HTMLButtonElement>('.menu-trigger')[index];
}
function items() {
  return Array.from(
    root.value?.querySelectorAll<HTMLButtonElement>('.menu-popup button:not(:disabled)') ?? [],
  );
}
function close(restoreFocus = false) {
  const wasOpen = opened.value !== null;
  opened.value = null;
  if (wasOpen && restoreFocus) trigger(activeIndex.value)?.focus();
}
async function show(index: number, last = false) {
  const menu = props.menus[index];
  if (!menu) return;
  if (
    opened.value === null &&
    document.activeElement instanceof HTMLElement &&
    !root.value?.contains(document.activeElement)
  )
    previousFocus = document.activeElement;
  activeIndex.value = index;
  opened.value = menu.id;
  await nextTick();
  if (opened.value !== menu.id) return;
  const available = items();
  // 无文档时视图菜单可能全部禁用，仍把焦点留在标题以便 Esc／左右键继续工作。
  ((last ? available.at(-1) : available[0]) ?? trigger(index))?.focus();
}
function toggle(index: number) {
  if (opened.value === props.menus[index]?.id) close(true);
  else void show(index);
}
function rememberFocus() {
  if (opened.value === null)
    previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
}
function groups(menu: WorkspaceMenu) {
  const result: WorkspaceMenuItem[][] = [];
  for (const item of menu.items) {
    if (!result.length || item.separatorBefore) result.push([]);
    result.at(-1)?.push(item);
  }
  return result;
}
function choose(item: WorkspaceMenuItem) {
  if (item.disabled) return;
  close();
  // 先回到工作面，再执行命令；文件对话框或面板操作可自行接管焦点。
  if (previousFocus?.isConnected) previousFocus.focus();
  else trigger(activeIndex.value)?.focus();
  emit('command', item.command);
}
function keydown(event: KeyboardEvent) {
  if (event.isComposing || event.repeat) return;
  if (event.ctrlKey || event.metaKey) {
    const command = workspaceShortcut(event);
    const item = props.menus.flatMap((menu) => menu.items).find((item) => item.command === command);
    if (item) {
      event.preventDefault();
      choose(item);
    }
    return;
  }
  if (event.altKey) {
    accessKey(event);
    return;
  }
  if (event.key === 'Escape') {
    event.preventDefault();
    close(true);
  } else if (event.key === 'Tab') {
    close(true);
  } else if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
    event.preventDefault();
    const next =
      (activeIndex.value + (event.key === 'ArrowRight' ? 1 : -1) + props.menus.length) %
      props.menus.length;
    if (opened.value !== null) void show(next);
    else {
      activeIndex.value = next;
      trigger(next)?.focus();
    }
  } else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
    event.preventDefault();
    if (
      opened.value === null ||
      (event.target instanceof Element && event.target.classList.contains('menu-trigger'))
    ) {
      void show(activeIndex.value, event.key === 'ArrowUp' || event.key === 'End');
      return;
    }
    const available = items();
    if (!available.length) return;
    const current = available.findIndex((item) => item === document.activeElement);
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? available.length - 1
          : (current + (event.key === 'ArrowDown' ? 1 : -1) + available.length) % available.length;
    available[next]?.focus();
  }
}
function outside(event: PointerEvent) {
  if (event.target instanceof Node && !root.value?.contains(event.target)) close();
}
function focusOutside(event: FocusEvent) {
  if (event.target instanceof Node && !root.value?.contains(event.target)) close();
}
function accessKey(event: KeyboardEvent) {
  if (
    !event.altKey ||
    event.ctrlKey ||
    event.metaKey ||
    event.shiftKey ||
    event.repeat ||
    event.isComposing ||
    event.defaultPrevented
  )
    return;
  const index = props.menus.findIndex(
    (menu) => menu.accessKey.toLowerCase() === event.key.toLowerCase(),
  );
  if (index < 0) return;
  event.preventDefault();
  void show(index);
}
function blur() {
  close();
}
onMounted(() => {
  document.addEventListener('pointerdown', outside);
  document.addEventListener('focusin', focusOutside);
  window.addEventListener('keydown', accessKey);
  window.addEventListener('blur', blur);
});
onUnmounted(() => {
  document.removeEventListener('pointerdown', outside);
  document.removeEventListener('focusin', focusOutside);
  window.removeEventListener('keydown', accessKey);
  window.removeEventListener('blur', blur);
});
</script>

<template>
  <nav
    ref="root"
    class="workspace-menubar"
    role="menubar"
    aria-label="应用菜单"
    @keydown.stop="keydown"
  >
    <div
      v-for="(menu, index) in menus"
      :key="menu.id"
      class="menu-group"
      role="none"
      @pointerenter="opened !== null && opened !== menu.id && show(index)"
    >
      <button
        :id="`menu-trigger-${menu.id}`"
        class="menu-trigger"
        role="menuitem"
        :aria-label="menu.label"
        aria-haspopup="menu"
        :aria-expanded="opened === menu.id"
        :aria-controls="`menu-popup-${menu.id}`"
        :aria-keyshortcuts="`Alt+${menu.accessKey}`"
        :tabindex="activeIndex === index ? 0 : -1"
        @focus="activeIndex = index"
        @pointerdown="rememberFocus"
        @click="toggle(index)"
      >
        {{ menu.label }}<span class="menu-access-key">({{ menu.accessKey }})</span>
      </button>
      <ul
        v-if="opened === menu.id"
        :id="`menu-popup-${menu.id}`"
        class="menu-popup"
        role="menu"
        :aria-labelledby="`menu-trigger-${menu.id}`"
      >
        <li v-for="(group, groupIndex) in groups(menu)" :key="groupIndex" role="none">
          <div v-if="groupIndex" class="menu-separator" role="separator"></div>
          <ul class="menu-items" role="group">
            <li v-for="item in group" :key="item.command" role="none">
              <button
                :role="item.checked === undefined ? 'menuitem' : 'menuitemradio'"
                :aria-label="item.label"
                :aria-checked="item.checked"
                :disabled="item.disabled"
                tabindex="-1"
                @click="choose(item)"
              >
                <span class="menu-check" aria-hidden="true">{{ item.checked ? '✓' : '' }}</span>
                <span>{{ item.label }}</span
                ><span class="menu-shortcut" aria-hidden="true">{{ item.shortcut }}</span>
              </button>
            </li>
          </ul>
        </li>
      </ul>
    </div>
  </nav>
</template>

<style scoped>
.workspace-menubar {
  display: flex;
  align-self: stretch;
  gap: 1px;
}
.menu-group {
  position: relative;
  display: flex;
  align-items: center;
}
.menu-trigger {
  border: 0;
  background: transparent;
  padding: 5px 9px;
  gap: 1px;
}
.menu-trigger[aria-expanded='true'] {
  background: #50535a;
  color: #fff;
}
.menu-access-key {
  color: var(--muted);
  font-size: 11px;
}
.menu-popup {
  position: absolute;
  z-index: 40;
  top: 100%;
  left: 0;
  width: 230px;
  max-height: calc(100vh - 100px);
  overflow: auto;
  list-style: none;
  margin: 0;
  padding: 5px 0;
  background: #35373c;
  border: 1px solid #60636c;
  border-radius: 0 0 4px 4px;
  box-shadow: 0 6px 18px #0008;
}
.menu-popup button {
  width: 100%;
  justify-content: flex-start;
  padding: 7px 12px;
  border: 0;
  border-radius: 0;
  background: transparent;
  font-size: 12px;
}
.menu-items {
  list-style: none;
  padding: 0;
  margin: 0;
}
.menu-popup button:hover:not(:disabled),
.menu-popup button:focus-visible {
  background: var(--accent-bg);
  outline: none;
  color: #fff;
}
.menu-check {
  width: 14px;
  flex-shrink: 0;
  color: var(--accent);
}
.menu-shortcut {
  margin-left: auto;
  font-size: 11px;
  color: var(--muted);
}
.menu-separator {
  height: 1px;
  background: #53565e;
  margin: 5px 12px;
}
</style>
