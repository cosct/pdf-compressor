<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type { AnalysisSummary } from '../types/pdf'
import { formatBytes, formatPercent } from '../utils/format'

const props = defineProps<{
  analysis: AnalysisSummary | null
  loading: boolean
  error: string
}>()

const { t } = useI18n()

const documentSummary = computed(() => {
  if (!props.analysis) {
    return {
      tone: 'neutral',
      title: t('analysis.states.waitingTitle'),
      body: t('analysis.states.waitingBody'),
    }
  }

  switch (props.analysis.documentKind) {
    case 'text-native':
      return {
        tone: 'warning',
        title: t('analysis.states.textNativeTitle'),
        body: t('analysis.states.textNativeBody'),
      }
    case 'scan-heavy':
      return {
        tone: 'success',
        title: t('analysis.states.scanHeavyTitle'),
        body: t('analysis.states.scanHeavyBody'),
      }
    default:
      return {
        tone: 'neutral',
        title: t('analysis.states.mixedTitle'),
        body: t('analysis.states.mixedBody'),
      }
  }
})

const facts = computed(() => {
  if (!props.analysis) {
    return []
  }

  return [
    { label: t('analysis.sourceSize'), value: formatBytes(props.analysis.fileSizeBytes) },
    { label: t('analysis.pageCount'), value: props.analysis.pageCount ?? '--' },
    { label: t('analysis.imageObjects'), value: props.analysis.imageObjectCount ?? '--' },
    {
      label: t('analysis.recommendedPreset'),
      value: props.analysis.recommendedPreset ? t(`app.preset.${props.analysis.recommendedPreset}`) : '--',
    },
  ]
})

const estimatedSavingsWidth = computed(() => {
  const percent = props.analysis?.estimatedSavingsPercent ?? 0
  const normalized = Math.max(0, Math.min(100, percent))
  return `${Math.max(normalized, props.analysis ? 10 : 0)}%`
})
</script>

<template>
  <section class="panel-surface report-panel" :aria-busy="props.loading ? 'true' : 'false'">
    <div class="section-header report-panel__header">
      <div>
        <p class="section-kicker">{{ t('analysis.eyebrow') }}</p>
        <h2>{{ t('analysis.title') }}</h2>
        <p class="report-panel__body">{{ t('analysis.body') }}</p>
      </div>
      <span v-if="props.analysis?.recommendedPreset" class="app-chip">
        {{ t('analysis.suggestedPrefix') }}: {{ t(`app.preset.${props.analysis.recommendedPreset}`) }}
      </span>
    </div>

    <div v-if="props.loading" class="state-box state-box--loading" role="status" aria-live="polite">
      <strong>{{ t('analysis.loadingTitle') }}</strong>
      <p>{{ t('analysis.loadingBody') }}</p>
    </div>

    <div v-else-if="props.error" class="state-box state-box--danger" role="alert">
      <strong>{{ t('analysis.errorTitle') }}</strong>
      <p>{{ props.error }}</p>
    </div>

    <div v-else-if="props.analysis" class="report-panel__content">
      <div class="analysis-stage">
        <article class="insight-card" :data-tone="documentSummary.tone">
          <p class="insight-card__eyebrow">{{ t('analysis.documentFit') }}</p>
          <strong>{{ documentSummary.title }}</strong>
          <p>{{ documentSummary.body }}</p>
        </article>

        <article class="savings-card">
          <p class="savings-card__eyebrow">{{ t('analysis.estimatedSavings') }}</p>
          <strong>{{ formatPercent(props.analysis.estimatedSavingsPercent) }}</strong>
          <div class="savings-card__track" aria-hidden="true">
            <span :style="{ width: estimatedSavingsWidth }"></span>
          </div>
        </article>
      </div>

      <dl class="fact-grid">
        <div v-for="fact in facts" :key="fact.label">
          <dt>{{ fact.label }}</dt>
          <dd>{{ fact.value }}</dd>
        </div>
      </dl>

      <div v-if="props.analysis.notes.length" class="notice-list">
        <article
          v-for="notice in props.analysis.notes.slice(0, 3)"
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
      <strong>{{ t('analysis.emptyTitle') }}</strong>
      <p>{{ t('analysis.emptyBody') }}</p>
    </div>
  </section>
</template>

<style scoped>
.report-panel,
.report-panel__content,
.analysis-stage,
.fact-grid,
.notice-list {
  display: grid;
  gap: var(--space-4);
}

.analysis-stage {
  grid-template-columns: minmax(0, 1.14fr) minmax(15rem, 0.86fr);
}

.report-panel__body,
.insight-card__eyebrow,
.fact-grid dt,
.notice-card p,
.savings-card__eyebrow {
  margin: 0;
  color: var(--color-ink-muted);
}

.insight-card,
.savings-card,
.fact-grid div,
.notice-card {
  border: 1px solid var(--color-line);
  border-radius: var(--radius-2);
  background: var(--color-surface-strong);
}

.insight-card,
.savings-card {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-4);
}

.insight-card[data-tone='success'] {
  background: var(--color-success-soft);
}

.insight-card[data-tone='warning'] {
  background: var(--color-warning-soft);
}

.insight-card__eyebrow,
.savings-card__eyebrow {
  font-size: var(--font-size-0);
  font-weight: 700;
  letter-spacing: 0.18em;
  text-transform: uppercase;
}

.savings-card {
  align-content: space-between;
  background:
    radial-gradient(circle at top right, rgba(120, 211, 203, 0.14), transparent 32%),
    linear-gradient(180deg, rgba(7, 17, 26, 0.48), rgba(18, 36, 54, 0.8));
}

.savings-card strong {
  color: var(--color-ink-strong);
  font-family: var(--font-display);
  font-size: clamp(2.4rem, 5vw, 3.6rem);
  line-height: 0.92;
}

.savings-card__track {
  width: 100%;
  height: 0.6rem;
  border-radius: var(--radius-pill);
  background: rgba(7, 17, 26, 0.54);
  overflow: hidden;
}

.savings-card__track span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: linear-gradient(90deg, var(--color-accent-secondary), var(--color-accent-strong));
  transition: width var(--transition-medium);
}

.fact-grid {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.fact-grid div {
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
.insight-card strong,
.notice-card strong {
  color: var(--color-ink-strong);
}

.fact-grid dd {
  font-weight: 600;
  overflow-wrap: anywhere;
}

.notice-list {
  gap: var(--space-3);
}

.notice-card {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-3);
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
  .analysis-stage,
  .fact-grid {
    grid-template-columns: repeat(1, minmax(0, 1fr));
  }
}
</style>
