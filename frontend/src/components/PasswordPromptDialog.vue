<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

/**
 * Retry prompt for password-protected PDFs. Shown when the selected job's
 * last backend error was `error.passwordRequired` / `error.wrongPassword`;
 * submitting re-runs the analysis with the supplied open password.
 * 加密 PDF 的密码重试弹窗：提交后携带密码重跑分析。
 */
const props = defineProps<{
  visible: boolean
  fileName: string
  /** True when a previously supplied password did not unlock the file. */
  wrongPassword: boolean
}>()

const emit = defineEmits<{
  submit: [password: string]
  dismiss: []
}>()

const { t } = useI18n()
const password = ref('')

const busyHint = computed(() =>
  props.wrongPassword ? t('password.wrongPasswordHint') : t('password.requiredHint'),
)

watch(
  () => props.visible,
  (visible) => {
    if (visible) {
      password.value = ''
    }
  },
)

function submit() {
  if (!password.value.trim()) {
    return
  }
  emit('submit', password.value)
}
</script>

<template>
  <div
    v-if="visible"
    class="password-overlay"
    role="dialog"
    aria-modal="true"
    :aria-label="t('password.title')"
    @keydown.esc="emit('dismiss')"
  >
    <div class="password-card fd-card">
      <h2 class="password-card__title">{{ t('password.title') }}</h2>
      <p class="password-card__file" :title="fileName">{{ fileName }}</p>
      <p class="password-card__hint">{{ busyHint }}</p>
      <form class="password-card__form" @submit.prevent="submit">
        <input
          ref="input"
          v-model="password"
          class="password-card__input"
          type="password"
          autocomplete="off"
          :placeholder="t('password.placeholder')"
          :aria-label="t('password.placeholder')"
          autofocus
        />
        <div class="password-card__actions">
          <button type="button" class="btn btn-ghost" @click="emit('dismiss')">
            {{ t('password.cancel') }}
          </button>
          <button type="submit" class="btn btn-primary" :disabled="!password.trim()">
            {{ t('password.submit') }}
          </button>
        </div>
      </form>
    </div>
  </div>
</template>

<style scoped>
.password-overlay {
  position: fixed;
  inset: 0;
  z-index: 90;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--fd-bg) 65%, transparent);
  backdrop-filter: blur(2px);
}

.password-card {
  width: min(380px, calc(100vw - 48px));
  padding: var(--fd-space-20);
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-8);
}

.password-card__title {
  margin: 0;
  font: var(--fd-text-subtitle);
}

.password-card__file {
  margin: 0;
  font: var(--fd-text-body);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.password-card__hint {
  margin: 0;
  font: var(--fd-text-caption);
  color: var(--fd-text-secondary);
}

.password-card__form {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-12);
}

.password-card__input {
  width: 100%;
  padding: 10px 12px;
  border-radius: var(--fd-radius-md);
  border: 1px solid var(--fd-border);
  background: var(--fd-surface);
  color: inherit;
  font: var(--fd-text-body);
}

.password-card__actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--fd-space-8);
}
</style>
