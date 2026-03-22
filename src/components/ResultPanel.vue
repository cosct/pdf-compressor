<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type { CompressionResult } from '../types/pdf'
import { directoryFromPath, formatBytes, formatMilliseconds, formatPercent } from '../utils/format'

const props = defineProps<{
  result: CompressionResult | null
  loading: boolean
  error: string
  canRun: boolean
  sourceReady: boolean
  analysisReady: boolean
}>()

const emit = defineEmits<{
  compress: []
}>()

const { t } = useI18n()

const resultSummary = computed(() => {
  if (!props.result) {
    return {
      tone: 'neutral',
      headline: '--',
      body: t('result.emptyBody'),
    }
  }

  if (props.result.outputWasSmaller === false) {
    return {
      tone: 'warning',
      headline: formatPercent(props.result.savingsPercent),
      body: t('result.unchangedBody'),
    }
  }

  return {
    tone: 'success',
    headline: formatPercent(props.result.savingsPercent),
    body: `${formatBytes(props.result.savedBytes)} ${t('result.savedSuffix')}`,
  }
})

const facts = computed(() => {
  if (!props.result) {
    return []
  }

  return [
    { label: t('result.original'), value: formatBytes(props.result.originalSizeBytes) },
    { label: t('result.compressed'), value: formatBytes(props.result.compressedSizeBytes) },
    { label: t('result.imagesOptimized'), value: props.result.imagesRecompressed ?? '--' },
    { label: t('result.imagesSkipped'), value: props.result.imagesSkipped ?? '--' },
    { label: t('result.streamsPacked'), value: props.result.streamsCompressed ?? '--' },
  ]
})

const savingsMeterWidth = computed(() => {
  const percent = props.result?.savingsPercent ?? 0
  const normalized = Math.max(0, Math.min(100, percent))
  return `${Math.max(normalized, props.result ? 10 : 0)}%`
})

const buttonCopy = computed(() => {
  if (props.loading) {
    return t('result.buttonBusy')
  }

  if (!props.sourceReady) {
    return t('result.buttonNeedsSource')
  }

  if (!props.analysisReady) {
    return t('result.buttonNeedsAnalysis')
  }

  return t('result.buttonIdle')
})
</script>

<template>
  <section class="panel-surface report-panel" :aria-busy="props.loading ? 'true' : 'false'">
    <div class="section-header report-panel__header">
      <div>
        <p class="section-kicker">{{ t('result.eyebrow') }}</p>
        <h2>{{ t('result.title') }}</h2>
        <p class="report-panel__body">{{ t('result.body') }}</p>
      </div>
      <button class="app-button app-button--primary" type="button" :disabled="!props.canRun || props.loading" :aria-disabled="!props.canRun || props.loading ? 'true' : 'false'" @click="emit('compress')">
        {{ buttonCopy }}
      </button>
    </div>

    <div v-if="props.loading" class="state-box state-box--loading" role="status" aria-live="polite">
      <strong>{{ t('result.loadingTitle') }}</strong>
      <p>{{ t('result.loadingBody') }}</p>
    </div>

    <div v-else-if="props.error" class="state-box state-box--danger" role="alert">
      <strong>{{ t('result.errorTitle') }}</strong>
      <p>{{ props.error }}</p>
    </div>

    <div v-else-if="props.result" class="report-panel__content">
      <div class="result-stage">
        <article class="result-hero" :data-tone="resultSummary.tone">
          <p class="result-hero__eyebrow">{{ t('result.sizeChange') }}</p>
          <strong>{{ resultSummary.headline }}</strong>
          <p>{{ resultSummary.body }}</p>
          <div class="result-hero__track" aria-hidden="true">
            <span :style="{ width: savingsMeterWidth }"></span>
          </div>
        </article>

        <article class="output-card">
          <p class="output-card__label">{{ t('result.outputLabel') }}</p>
          <strong class="output-card__path" :title="props.result.outputPath">{{ props.result.outputPath }}</strong>
          <div class="output-card__meta">
            <span>{{ directoryFromPath(props.result.outputPath) }}</span>
            <span>{{ t('result.elapsedLabel') }}: {{ formatMilliseconds(props.result.elapsedMs) }}</span>
          </div>
        </article>
      </div>

      <dl class="fact-grid">
        <div v-for="fact in facts" :key="fact.label">
          <dt>{{ fact.label }}</dt>
          <dd>{{ fact.value }}</dd>
        </div>
      </dl>

      <div v-if="props.result.notes.length" class="notice-list">
        <article
          v-for="notice in props.result.notes.slice(0, 3)"
          :key="notice.id"
          class="notice-card"
          :data-tone="notice.tone"
        >
          <strong>{{ notice.title }}</strong>
          <p>{{ notice.body }}</p>
        </article>
      </div>
    </div>

    <div v-else class="state-box">
      <strong>{{ t('result.emptyTitle') }}</strong>
      <p>{{ t('result.emptyBody') }}</p>
    </div>
  </section>
</template>

<style scoped>
.report-panel,
.report-panel__content,
.result-stage,
.fact-grid,
.notice-list {
  display: grid;
  gap: var(--space-4);
}

.result-stage {
  grid-template-columns: minmax(0, 1fr) minmax(16rem, 0.88fr);
}

.report-panel__body,
.result-hero__eyebrow,
.fact-grid dt,
.output-card__label,
.output-card span,
.notice-card p {
  margin: 0;
  color: var(--color-ink-muted);
}

.result-hero,
.fact-grid div,
.output-card,
.notice-card {
  border: 1px solid var(--color-line);
  border-radius: var(--radius-2);
  background: var(--color-surface-strong);
}

.result-hero {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-4);
}

.result-hero[data-tone='success'] {
  background:
    radial-gradient(circle at top right, rgba(73, 195, 145, 0.16), transparent 28%),
    rgba(73, 195, 145, 0.1);
}

.result-hero[data-tone='warning'] {
  background:
    radial-gradient(circle at top right, rgba(242, 186, 103, 0.16), transparent 28%),
    rgba(242, 186, 103, 0.1);
}

.result-hero__eyebrow,
.output-card__label {
  font-size: var(--font-size-0);
  font-weight: 700;
  letter-spacing: 0.18em;
  text-transform: uppercase;
}

.result-hero strong {
  color: var(--color-ink-strong);
  font-family: var(--font-display);
  font-size: clamp(2.8rem, 6vw, 4.4rem);
  line-height: 0.92;
}

.result-hero__track {
  width: 100%;
  height: 0.6rem;
  border-radius: var(--radius-pill);
  background: rgba(7, 17, 26, 0.52);
  overflow: hidden;
}

.result-hero__track span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: linear-gradient(90deg, var(--color-accent-secondary), var(--color-accent-strong));
  transition: width var(--transition-medium);
}

.fact-grid {
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.fact-grid div,
.output-card,
.notice-card {
  padding: var(--space-3);
}

.fact-grid dt,
.fact-grid dd {
  margin: 0;
}

.fact-grid dt {
  margin-bottom: var(--space-1);
  font-size: var(--font-size-1);
}

.fact-grid dd,
.output-card strong,
.notice-card strong {
  color: var(--color-ink-strong);
}

.fact-grid dd {
  font-weight: 600;
}

.output-card {
  display: grid;
  gap: var(--space-2);
  background: linear-gradient(180deg, rgba(7, 17, 26, 0.42), rgba(18, 36, 54, 0.78));
}

.output-card__path {
  display: block;
  line-height: 1.35;
  overflow-wrap: anywhere;
}

.output-card__meta {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-1) var(--space-3);
}

.notice-list {
  gap: var(--space-3);
}

.notice-card {
  display: grid;
  gap: var(--space-2);
}

.notice-card[data-tone='warning'] {
  background: var(--color-warning-soft);
}

.notice-card[data-tone='success'] {
  background: var(--color-success-soft);
}

.notice-card[data-tone='danger'] {
  background: var(--color-danger-soft);
}

@media (max-width: 62rem) {
  .result-stage,
  .fact-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .report-panel__header {
    display: grid;
  }
}

@media (max-width: 48rem) {
  .result-stage,
  .fact-grid {
    grid-template-columns: repeat(1, minmax(0, 1fr));
  }
}
</style>
