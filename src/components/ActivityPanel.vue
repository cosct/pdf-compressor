<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type {
  AnalysisSummary,
  CompressionPreset,
  CompressionResult,
  WorkflowState,
} from '../types/pdf'
import { formatPercent } from '../utils/format'

const props = withDefaults(
  defineProps<{
    workflowState: WorkflowState
    selectedFileName: string
    selectedPreset: CompressionPreset | null
    recommendedPreset: CompressionPreset | null
    analysis: AnalysisSummary | null
    result: CompressionResult | null
    primaryActionLabel: string
    primaryActionDisabled: boolean
    canCancel: boolean
    queueCount: number
    completedCount: number
    progressPercent?: number
    outputDir?: string | null
    nativeAvailable?: boolean
    queueLocked?: boolean
  }>(),
  {
    selectedPreset: null,
    progressPercent: 0,
    outputDir: null,
    nativeAvailable: false,
    queueLocked: false,
  },
)

const emit = defineEmits<{
  'primary-action': []
  'cancel-action': []
  'select-output-dir': []
}>()

const { t } = useI18n()

const statusCopy = computed(() => {
  switch (props.workflowState) {
    case 'analyzing':
      return { tone: 'accent', title: t('activity.states.analyzingTitle') }
    case 'compressing':
      return { tone: 'accent', title: t('activity.states.compressingTitle') }
    case 'ready':
      return { tone: 'success', title: t('activity.states.readyTitle') }
    case 'success':
      return { tone: 'success', title: t('activity.states.successTitle') }
    case 'error':
      return { tone: 'danger', title: t('activity.states.errorTitle') }
    case 'selected':
      return { tone: 'neutral', title: t('activity.states.selectedTitle') }
    default:
      return { tone: 'neutral', title: t('activity.states.idleTitle') }
  }
})

const metrics = computed(() => [
  {
    label: t('activity.metricSavings'),
    value: formatPercent(props.result?.savingsPercent ?? props.analysis?.estimatedSavingsPercent),
  },
  {
    label: t('activity.metricProgress'),
    value: `${Math.round(props.progressPercent ?? 0)}%`,
  },
  {
    label: t('activity.metricQueued'),
    value: `${props.queueCount}`,
  },
  {
    label: t('activity.metricCompleted'),
    value: `${props.completedCount}`,
  },
])
</script>

<template>
  <section class="activity-panel" :data-state="props.workflowState">
    <div class="activity-head">
      <div class="panel-header">
        <svg class="panel-header__icon" width="20" height="20" viewBox="0 0 20 20" fill="none" aria-hidden="true">
          <path d="M3 10h2l2-5 3 10 2-5h5" stroke="var(--fd-accent)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
        <h2>{{ t('activity.eyebrow') }}</h2>
      </div>
      <span v-if="props.queueLocked" class="fd-badge fd-badge--accent">{{ t('upload.lockedTag') }}</span>
    </div>

    <div class="activity-grid">
      <div class="status-card" :class="`status-card--${statusCopy.tone}`">
        <div class="status-card__header">
          <span class="fd-badge" :class="`fd-badge--${statusCopy.tone}`">
            {{ t('activity.liveLabel') }}
          </span>
          <strong>{{ statusCopy.title }}</strong>
        </div>

        <div class="status-card__chips">
          <div v-if="props.selectedFileName" class="status-card__file">
            <span class="fd-badge">{{ props.selectedFileName }}</span>
          </div>
          <div
            v-if="props.selectedPreset || (props.recommendedPreset && props.recommendedPreset !== props.selectedPreset)"
            class="status-card__tags"
          >
            <span v-if="props.selectedPreset" class="fd-badge fd-badge--accent">
              {{ t(`app.preset.${props.selectedPreset}`) }}
            </span>
            <span
              v-if="props.recommendedPreset && props.recommendedPreset !== props.selectedPreset"
              class="fd-badge"
            >
              {{ t(`app.preset.${props.recommendedPreset}`) }}
            </span>
          </div>
        </div>

        <div
          class="fd-progress"
          role="progressbar"
          :aria-label="statusCopy.title"
          :aria-valuemin="0"
          :aria-valuemax="100"
          :aria-valuenow="Math.round(props.progressPercent ?? 0)"
        >
          <span
            class="fd-progress__bar"
            :style="{ width: `${Math.max(props.progressPercent ?? 0, props.workflowState === 'success' ? 100 : 2)}%` }"
          ></span>
        </div>
      </div>

      <div class="output-dir">
        <span class="output-dir__label">{{ t('settings.outputDir') }}</span>
        <div class="output-dir__row">
          <span class="output-dir__path" :title="props.outputDir || t('settings.outputDirDefault')">
            {{ props.outputDir || t('settings.outputDirDefault') }}
          </span>
          <button
            class="fd-button fd-button--subtle output-dir__btn"
            type="button"
            :disabled="props.queueLocked || !props.nativeAvailable"
            @click="emit('select-output-dir')"
          >
            {{ t('settings.outputDirBrowse') }}
          </button>
        </div>
        <p v-if="props.queueLocked" class="output-dir__hint">{{ t('activity.outputLocked') }}</p>
      </div>

      <div class="action-block">
        <div v-if="props.canCancel" class="action-single">
          <button
            class="fd-button cancel-btn cancel-btn--center"
            type="button"
            @click="emit('cancel-action')"
          >
            {{ t('activity.cancel') }}
          </button>
        </div>
        <div v-else-if="props.queueCount > 0" class="action-single">
          <button
            class="fd-button fd-button--accent compress-btn"
            type="button"
            :disabled="props.primaryActionDisabled"
            @click="emit('primary-action')"
          >
            <svg v-if="!props.primaryActionDisabled" width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
              <path d="M4 8h8M8 4v8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
            </svg>
            {{ props.primaryActionLabel }}
          </button>
        </div>
        <div v-else class="action-empty">
          {{ t('activity.noSourcePlaceholder') }}
        </div>
      </div>

      <dl class="metrics-grid">
        <div v-for="metric in metrics" :key="metric.label" class="metric-card">
          <dt>{{ metric.label }}</dt>
          <dd>{{ metric.value }}</dd>
        </div>
      </dl>
    </div>
  </section>
</template>

<style scoped>
.activity-panel {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-10);
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
}

.activity-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fd-space-10);
}

.panel-header {
  display: flex;
  align-items: center;
  gap: var(--fd-space-8);
}

.panel-header__icon {
  flex-shrink: 0;
}

.panel-header h2 {
  font: var(--fd-text-section);
}

.activity-grid {
  display: grid;
  flex: 1;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  grid-template-rows: auto minmax(0, 1fr);
  grid-template-areas:
    'status output'
    'action metrics';
  gap: var(--fd-space-8);
  min-height: 0;
  align-items: stretch;
}

.status-card {
  grid-area: status;
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-8);
  padding: 12px;
  border: 1px solid var(--fd-stroke-card);
  border-radius: 18px;
  background: color-mix(in srgb, var(--fd-layer-2) 90%, transparent);
}

.status-card--accent {
  background: var(--fd-accent-subtle);
  border-color: var(--fd-accent-border);
}

.status-card--success {
  background: var(--fd-success-subtle);
  border-color: var(--fd-success-border);
}

.status-card--danger {
  background: var(--fd-danger-subtle);
  border-color: var(--fd-danger-border);
}

.status-card__header {
  display: flex;
  align-items: center;
  gap: var(--fd-space-8);
}

.status-card__header strong {
  min-width: 0;
  font: var(--fd-text-body-strong);
  color: var(--fd-text-primary);
}

.status-card__chips {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-6);
}

.status-card__file,
.status-card__tags {
  display: flex;
  flex-wrap: wrap;
  gap: var(--fd-space-6);
}

.status-card__chips .fd-badge {
  white-space: normal;
  line-height: 1.25;
  word-break: break-word;
}

.status-card__file .fd-badge {
  max-width: 100%;
}

.output-dir {
  grid-area: output;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  gap: var(--fd-space-6);
  padding: 12px;
  border: 1px solid var(--fd-stroke-card);
  border-radius: 18px;
  background: color-mix(in srgb, var(--fd-layer-2) 90%, transparent);
}

.output-dir__label {
  font: var(--fd-text-caption);
  font-weight: 500;
  color: var(--fd-text-tertiary);
}

.output-dir__row {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: var(--fd-space-8);
}

.output-dir__path {
  font: var(--fd-text-caption);
  color: var(--fd-text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.output-dir__btn {
  width: 100%;
  min-height: 30px;
  padding: 0 10px;
  font: var(--fd-text-caption);
}

.output-dir__hint {
  color: var(--fd-text-tertiary);
  font: var(--fd-text-caption);
}

.action-block {
  grid-area: action;
  display: flex;
  flex-direction: column;
  min-height: 0;
  gap: var(--fd-space-8);
}

.action-single {
  display: flex;
  flex: 1;
  align-items: center;
  justify-content: center;
  min-height: 0;
}

.action-empty {
  display: flex;
  flex: 1;
  align-items: center;
  justify-content: center;
  min-height: 0;
  padding: 12px;
  border: 1px dashed var(--fd-stroke-card);
  border-radius: 16px;
  color: var(--fd-text-tertiary);
  font: var(--fd-text-body-strong);
  text-align: center;
}

.compress-btn {
  width: min(100%, 220px);
  min-height: 50px;
  border-radius: 16px;
  font: var(--fd-text-body-strong);
  gap: var(--fd-space-6);
  white-space: normal;
  text-align: center;
}

.cancel-btn {
  border-radius: 14px;
  background: var(--fd-danger-subtle);
  border-color: var(--fd-danger-border);
  color: var(--fd-danger);
}

.cancel-btn--center {
  width: min(100%, 220px);
  min-height: 46px;
}

.metrics-grid {
  grid-area: metrics;
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--fd-space-8);
  height: 100%;
  margin: 0;
  padding: 0;
}

.metric-card {
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: var(--fd-space-2);
  min-height: 0;
  padding: 10px 12px;
  border: 1px solid var(--fd-stroke-card);
  border-radius: 16px;
  background: color-mix(in srgb, var(--fd-layer-2) 88%, transparent);
  text-align: left;
}

.metric-card dt {
  margin: 0;
  font: var(--fd-text-caption);
  color: var(--fd-text-tertiary);
}

.metric-card dd {
  margin: 0;
  font: 600 16px/20px var(--fd-font-family);
  color: var(--fd-text-primary);
}

@media (max-width: 900px) {
  .activity-grid {
    grid-template-columns: 1fr;
    grid-template-areas:
      'status'
      'output'
      'action'
      'metrics';
  }

  .output-dir__path {
    white-space: normal;
    overflow: visible;
    text-overflow: clip;
  }
}
</style>
