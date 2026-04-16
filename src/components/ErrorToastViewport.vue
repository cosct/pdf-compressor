<script setup lang="ts">
import { useI18n } from 'vue-i18n'

import type { NoticeItem } from '../types/pdf'

const props = defineProps<{
  items: NoticeItem[]
}>()

const emit = defineEmits<{
  dismiss: [id: string]
}>()

const { t } = useI18n()

function toneClass(tone: NoticeItem['tone']) {
  return `toast-card--${tone}`
}
</script>

<template>
  <aside v-if="props.items.length" class="toast-viewport" aria-live="polite" aria-atomic="false">
    <article
      v-for="item in props.items"
      :key="item.id"
      class="toast-card"
      :class="toneClass(item.tone)"
      role="alert"
    >
      <div class="toast-card__body">
        <strong>{{ item.title }}</strong>
        <p>{{ item.body }}</p>
      </div>

      <button
        class="toast-card__close"
        type="button"
        :title="t('header.close')"
        :aria-label="t('header.close')"
        @click="emit('dismiss', item.id)"
      >
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
          <path d="M3 3l6 6M9 3 3 9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
        </svg>
      </button>
    </article>
  </aside>
</template>

<style scoped>
.toast-viewport {
  position: fixed;
  top: calc(env(titlebar-area-height, 0px) + 16px);
  right: 16px;
  z-index: 80;
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: min(360px, calc(100vw - 24px));
  pointer-events: none;
}

.toast-card {
  pointer-events: auto;
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 10px 12px;
  align-items: start;
  padding: 14px 14px 14px 16px;
  border-radius: 18px;
  border: 1px solid var(--fd-stroke-card);
  background: color-mix(in srgb, var(--fd-layer-2) 94%, transparent);
  box-shadow: 0 18px 48px rgba(15, 23, 42, 0.18);
  backdrop-filter: blur(18px);
}

.toast-card--danger {
  border-color: var(--fd-danger-border);
  background: color-mix(in srgb, var(--fd-danger-subtle) 82%, var(--fd-layer-2));
}

.toast-card--warning {
  border-color: var(--fd-warning-border);
  background: color-mix(in srgb, var(--fd-warning-subtle) 84%, var(--fd-layer-2));
}

.toast-card--success {
  border-color: var(--fd-success-border);
  background: color-mix(in srgb, var(--fd-success-subtle) 84%, var(--fd-layer-2));
}

.toast-card__body {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.toast-card__body strong {
  font: var(--fd-text-body-strong);
  color: var(--fd-text-primary);
}

.toast-card__body p {
  margin: 0;
  font: var(--fd-text-caption);
  line-height: 1.45;
  color: var(--fd-text-secondary);
  word-break: break-word;
}

.toast-card__close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: 1px solid transparent;
  border-radius: 999px;
  background: transparent;
  color: var(--fd-text-secondary);
  cursor: pointer;
  transition:
    background var(--fd-duration-fast) var(--fd-easing-standard),
    color var(--fd-duration-fast) var(--fd-easing-standard),
    border-color var(--fd-duration-fast) var(--fd-easing-standard);
}

.toast-card__close:hover {
  background: color-mix(in srgb, var(--fd-layer-0) 72%, transparent);
  border-color: var(--fd-stroke-soft);
  color: var(--fd-text-primary);
}

.toast-card__close:focus-visible {
  outline: 2px solid var(--fd-accent);
  outline-offset: 2px;
}

@media (max-width: 768px) {
  .toast-viewport {
    top: calc(env(titlebar-area-height, 0px) + 12px);
    right: 12px;
    width: min(100%, calc(100vw - 24px));
  }
}
</style>
