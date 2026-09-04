<script setup lang="ts">
/**
 * Themed unit dropdown — replaces the native `<select>`, whose popup is
 * rendered by the OS/GTK theme and ignores the app's dark/light tokens.
 * Options render in an app-styled flyout; keyboard + outside-click handled.
 */
import { onBeforeUnmount, onMounted, ref } from 'vue'

const props = withDefaults(
  defineProps<{
    modelValue: string
    options: readonly string[]
    disabled?: boolean
    ariaLabel?: string
  }>(),
  { disabled: false, ariaLabel: undefined },
)

const emit = defineEmits<{
  'update:modelValue': [value: string]
}>()

const open = ref(false)
const root = ref<HTMLElement | null>(null)
const activeIndex = ref(0)

function choose(option: string) {
  emit('update:modelValue', option)
  open.value = false
}

function toggle() {
  if (props.disabled) {
    return
  }
  open.value = !open.value
  if (open.value) {
    activeIndex.value = Math.max(0, props.options.indexOf(props.modelValue))
  }
}

function selectAt(index: number) {
  const option = props.options[index]
  if (option != null) {
    choose(option)
  }
}

function onKeydown(event: KeyboardEvent) {
  if (!open.value) {
    return
  }
  switch (event.key) {
    case 'Escape':
      open.value = false
      break
    case 'ArrowDown':
      event.preventDefault()
      activeIndex.value = Math.min(activeIndex.value + 1, props.options.length - 1)
      break
    case 'ArrowUp':
      event.preventDefault()
      activeIndex.value = Math.max(activeIndex.value - 1, 0)
      break
    case 'Enter':
    case ' ':
      event.preventDefault()
      selectAt(activeIndex.value)
      break
  }
}

function onDocumentPointerDown(event: MouseEvent) {
  if (root.value && event.target instanceof Node && !root.value.contains(event.target)) {
    open.value = false
  }
}

onMounted(() => document.addEventListener('pointerdown', onDocumentPointerDown))
onBeforeUnmount(() => document.removeEventListener('pointerdown', onDocumentPointerDown))
</script>

<template>
  <div ref="root" class="unit-select" @keydown="onKeydown">
    <button
      type="button"
      class="unit-select__trigger"
      :disabled="props.disabled"
      :aria-expanded="open ? 'true' : 'false'"
      aria-haspopup="listbox"
      :aria-label="props.ariaLabel"
      @click="toggle"
    >
      {{ props.modelValue }}
      <svg
        class="unit-select__chevron"
        width="8"
        height="6"
        viewBox="0 0 8 6"
        fill="none"
        aria-hidden="true"
      >
        <path
          d="M1 1.5 4 4.5 7 1.5"
          stroke="currentColor"
          stroke-width="1.3"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>

    <ul v-if="open" class="unit-select__list" role="listbox" :aria-label="props.ariaLabel">
      <li v-for="(option, index) in props.options" :key="option">
        <button
          type="button"
          role="option"
          class="unit-select__option"
          :class="{
            'unit-select__option--active': index === activeIndex,
            'unit-select__option--selected': option === props.modelValue,
          }"
          :aria-selected="option === props.modelValue ? 'true' : 'false'"
          @click="choose(option)"
          @mouseenter="activeIndex = index"
        >
          {{ option }}
        </button>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.unit-select {
  position: relative;
  display: flex;
  align-self: stretch;
}

.unit-select__trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fd-space-6);
  width: 100%;
  min-height: var(--fd-control-height-sm);
  padding: 0 var(--fd-space-10);
  border: none;
  border-left: 1px solid var(--fd-control-stroke);
  /* Round the right side to match the entry container (which cannot use
     overflow: hidden — it would clip the popup). */
  border-radius: 0 calc(var(--fd-radius-sm) - 1px) calc(var(--fd-radius-sm) - 1px) 0;
  background: transparent;
  color: var(--fd-text-primary);
  font: var(--fd-text-body);
  cursor: pointer;
  outline: none;
}

.unit-select__trigger:disabled {
  cursor: not-allowed;
  opacity: 0.5;
}

.unit-select__trigger:focus-visible {
  box-shadow: inset 0 0 0 2px var(--fd-accent-border);
}

.unit-select__chevron {
  flex-shrink: 0;
  color: var(--fd-text-tertiary);
  transition: transform var(--fd-duration-fast) var(--fd-easing-standard);
}

.unit-select__trigger[aria-expanded='true'] .unit-select__chevron {
  transform: rotate(180deg);
}

.unit-select__list {
  position: absolute;
  top: calc(100% + 4px);
  right: 0;
  z-index: 30;
  min-width: 100%;
  margin: 0;
  padding: 4px;
  list-style: none;
  border: 1px solid var(--fd-stroke-card);
  border-radius: var(--fd-radius-sm);
  background: var(--fd-flyout-bg);
  box-shadow: var(--fd-shadow-lg);
  backdrop-filter: blur(24px) saturate(150%);
  -webkit-backdrop-filter: blur(24px) saturate(150%);
}

.unit-select__option {
  display: flex;
  align-items: center;
  width: 100%;
  min-height: 30px;
  padding: 0 var(--fd-space-8);
  border: none;
  border-radius: calc(var(--fd-radius-sm) - 2px);
  background: transparent;
  color: var(--fd-text-secondary);
  cursor: pointer;
  font: var(--fd-text-caption);
  text-align: left;
  white-space: nowrap;
}

.unit-select__option--active {
  background: var(--fd-subtle-bg-hover);
  color: var(--fd-text-primary);
}

.unit-select__option--selected {
  color: var(--fd-accent);
  font-weight: 600;
}
</style>
