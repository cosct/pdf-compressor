<script setup lang="ts">
/**
 * Output directory row — pinned to the top of the main-view rail so the
 * destination is chosen before anything runs.
 * 输出目录 —— 固定在主视图侧栏顶部，先定去向再开始压缩。
 */
import { useI18n } from 'vue-i18n'

const props = withDefaults(
  defineProps<{
    outputDir?: string | null
    nativeAvailable?: boolean
    queueLocked?: boolean
  }>(),
  {
    outputDir: null,
    nativeAvailable: false,
    queueLocked: false,
  },
)

const emit = defineEmits<{
  select: []
}>()

const { t } = useI18n()
</script>

<template>
  <section class="output-dir" :aria-label="t('settings.outputDir')">
    <div class="output-dir__row">
      <span class="output-dir__label">{{ t('settings.outputDir') }}</span>
      <button
        class="fd-button fd-button--subtle output-dir__btn"
        type="button"
        :disabled="props.queueLocked || !props.nativeAvailable"
        :title="props.queueLocked ? t('activity.outputLocked') : props.nativeAvailable ? undefined : t('activity.outputDirDesktopOnly')"
        @click="emit('select')"
      >
        {{ t('settings.outputDirBrowse') }}
      </button>
    </div>
    <span class="output-dir__path" :title="props.outputDir || t('settings.outputDirDefault')">
      {{ props.outputDir || t('settings.outputDirDefault') }}
    </span>
  </section>
</template>

<style scoped>
.output-dir {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-6);
  padding-bottom: var(--fd-space-12);
  border-bottom: 1px solid var(--fd-stroke-soft);
}

.output-dir__row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fd-space-8);
}

.output-dir__label {
  font: var(--fd-text-caption);
  font-weight: 600;
  color: var(--fd-text-secondary);
}

.output-dir__path {
  font: var(--fd-text-caption);
  color: var(--fd-text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.output-dir__btn {
  min-height: var(--fd-control-height-xs);
  padding: 0 var(--fd-space-10);
  font: var(--fd-text-caption);
}

@media (max-width: 900px) {
  .output-dir__path {
    white-space: normal;
    overflow: visible;
    text-overflow: clip;
  }
}
</style>
