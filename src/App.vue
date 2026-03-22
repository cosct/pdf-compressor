<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import AnalysisPanel from './components/AnalysisPanel.vue'
import AppHeader from './components/AppHeader.vue'
import CompressionSettingsPanel from './components/CompressionSettingsPanel.vue'
import FileIntakePanel from './components/FileIntakePanel.vue'
import ResultPanel from './components/ResultPanel.vue'
import { appLocales, setAppLocale, type AppLocale } from './i18n'
import { usePdfCompressor } from './composables/usePdfCompressor'
import { formatBytes, formatPercent } from './utils/format'

const {
  sourcePath,
  sourceFileName,
  settings,
  analysis,
  result,
  nativeAvailable,
  sourcePathState,
  analysisLoading,
  compressionLoading,
  analysisError,
  compressionError,
  workflowState,
  canCompress,
  hasCompletedAnalysis,
  setSourcePath,
  updateSettings,
  browseForPdf,
  analyzeCurrentPdf,
  compressCurrentPdf,
} = usePdfCompressor()

const { t, locale } = useI18n()

const busy = computed(() => analysisLoading.value || compressionLoading.value)
const sourceReady = computed(() => sourcePath.value.trim().toLowerCase().endsWith('.pdf'))

const statusCopy = computed(() => {
  switch (workflowState.value) {
    case 'analyzing':
      return {
        eyebrow: t('app.status.analyzing.eyebrow'),
        title: t('app.status.analyzing.title'),
        body: t('app.status.analyzing.body'),
      }
    case 'compressing':
      return {
        eyebrow: t('app.status.compressing.eyebrow'),
        title: t('app.status.compressing.title'),
        body: t('app.status.compressing.body'),
      }
    case 'ready':
      return {
        eyebrow: t('app.status.ready.eyebrow'),
        title: t('app.status.ready.title'),
        body: t('app.status.ready.body'),
      }
    case 'success':
      return {
        eyebrow: t('app.status.success.eyebrow'),
        title: t('app.status.success.title'),
        body: t('app.status.success.body'),
      }
    case 'error':
      return {
        eyebrow: t('app.status.error.eyebrow'),
        title: t('app.status.error.title'),
        body: t('app.status.error.body'),
      }
    case 'selected':
      return {
        eyebrow: t('app.status.selected.eyebrow'),
        title: t('app.status.selected.title'),
        body: t('app.status.selected.body'),
      }
    default:
      return {
        eyebrow: t('app.status.idle.eyebrow'),
        title: t('app.status.idle.title'),
        body: t('app.status.idle.body'),
      }
  }
})

const profileLabel = computed(() => t(`app.preset.${settings.preset}`))
const currentError = computed(() => compressionError.value || analysisError.value)
const sidebarNotices = computed(() => (result.value?.notes ?? analysis.value?.notes ?? []).slice(0, 2))
const expectedGain = computed(() => {
  if (result.value?.savingsPercent !== undefined) {
    return formatPercent(result.value.savingsPercent)
  }

  return formatPercent(analysis.value?.estimatedSavingsPercent)
})

const statusFacts = computed(() => [
  {
    label: t('app.source'),
    value: sourceFileName.value || t('app.emptySource'),
  },
  {
    label: t('app.profile'),
    value: profileLabel.value,
  },
  {
    label: t('app.expected'),
    value: expectedGain.value,
  },
  {
    label: t('app.size'),
    value: formatBytes(analysis.value?.fileSizeBytes),
  },
])

const workflowSteps = computed(() => [
  {
    id: 'intake',
    number: '01',
    title: t('app.steps.intake.title'),
    body: t('app.steps.intake.body'),
    active: workflowState.value !== 'idle',
  },
  {
    id: 'review',
    number: '02',
    title: t('app.steps.review.title'),
    body: t('app.steps.review.body'),
    active: ['ready', 'compressing', 'success'].includes(workflowState.value),
  },
  {
    id: 'export',
    number: '03',
    title: t('app.steps.export.title'),
    body: t('app.steps.export.body'),
    active: ['compressing', 'success'].includes(workflowState.value),
  },
])

const trustPoints = computed(() => [
  {
    title: t('app.trust.localTitle'),
    body: t('app.trust.localBody'),
  },
  {
    title: t('app.trust.guidedTitle'),
    body: t('app.trust.guidedBody'),
  },
  {
    title: t('app.trust.reviewTitle'),
    body: t('app.trust.reviewBody'),
  },
])

function clearSource() {
  setSourcePath('')
}

function updateLocale(nextLocale: AppLocale) {
  setAppLocale(nextLocale)
}
</script>

<template>
  <div class="app-shell" :aria-busy="busy ? 'true' : 'false'">
    <section class="hero-stage">
      <AppHeader
        :file-name="sourceFileName"
        :native-available="nativeAvailable"
        :locale="locale as AppLocale"
        :locales="appLocales"
        @update:locale="updateLocale"
      />

      <FileIntakePanel
        :model-value="sourcePath"
        :busy="busy"
        :native-available="nativeAvailable"
        :path-tone="sourcePathState.tone"
        :path-message="sourcePathState.message"
        @update:model-value="setSourcePath"
        @browse="browseForPdf"
        @analyze="analyzeCurrentPdf"
        @clear="clearSource"
      />
    </section>

    <section class="overview-stage">
      <section
        class="panel-surface status-card"
        :data-state="workflowState"
        role="status"
        aria-live="polite"
        aria-atomic="true"
      >
        <div class="status-card__header">
          <div>
            <p class="section-kicker">{{ t('app.workflow') }}</p>
            <h2>{{ statusCopy.title }}</h2>
            <p class="status-card__body">{{ statusCopy.body }}</p>
          </div>
          <span class="app-chip app-chip--accent status-card__profile">{{ profileLabel }}</span>
        </div>

        <dl class="status-card__facts">
          <div v-for="fact in statusFacts" :key="fact.label">
            <dt>{{ fact.label }}</dt>
            <dd>{{ fact.value }}</dd>
          </div>
        </dl>

        <div v-if="currentError" class="inline-alert inline-alert--danger" role="alert">
          <strong>{{ t('app.alertTitle') }}</strong>
          <p>{{ currentError }}</p>
        </div>

        <div v-else-if="sidebarNotices.length" class="status-card__notices">
          <article
            v-for="notice in sidebarNotices"
            :key="notice.id"
            class="notice-snippet"
            :data-tone="notice.tone"
          >
            <strong>{{ notice.title }}</strong>
            <p>{{ notice.body }}</p>
          </article>
        </div>

        <p v-else class="status-card__eyebrow">{{ statusCopy.eyebrow }}</p>
      </section>

      <div class="overview-stage__aside">
        <section class="panel-surface workflow-panel">
          <div>
            <p class="section-kicker">{{ t('app.workflow') }}</p>
            <h2>{{ t('app.stepsTitle') }}</h2>
            <p class="workflow-panel__body">{{ t('app.stepsBody') }}</p>
          </div>

          <ol class="workflow-panel__list">
            <li
              v-for="step in workflowSteps"
              :key="step.id"
              class="workflow-panel__step"
              :class="{ 'workflow-panel__step--active': step.active }"
            >
              <span class="workflow-panel__number">{{ step.number }}</span>
              <div>
                <strong>{{ step.title }}</strong>
                <p>{{ step.body }}</p>
              </div>
            </li>
          </ol>
        </section>

        <section class="panel-surface trust-panel">
          <div>
            <p class="section-kicker">{{ t('app.trustKicker') }}</p>
            <h2>{{ t('app.trustTitle') }}</h2>
            <p class="trust-panel__body">{{ t('app.trustBody') }}</p>
          </div>

          <div class="trust-panel__list">
            <article v-for="point in trustPoints" :key="point.title" class="trust-panel__item">
              <strong>{{ point.title }}</strong>
              <p>{{ point.body }}</p>
            </article>
          </div>
        </section>
      </div>
    </section>

    <section class="tool-stage">
      <CompressionSettingsPanel
        :settings="settings"
        :disabled="busy || !hasCompletedAnalysis"
        :recommended-preset="analysis?.recommendedPreset"
        @update:settings="updateSettings"
      />

      <div class="tool-stage__reports">
        <AnalysisPanel :analysis="analysis" :loading="analysisLoading" :error="analysisError" />

        <ResultPanel
          :result="result"
          :loading="compressionLoading"
          :error="compressionError"
          :can-run="canCompress"
          :source-ready="sourceReady"
          :analysis-ready="hasCompletedAnalysis"
          @compress="compressCurrentPdf"
        />
      </div>
    </section>
  </div>
</template>

<style scoped>
.app-shell,
.overview-stage__aside,
.tool-stage,
.status-card,
.status-card__facts,
.status-card__notices,
.workflow-panel,
.trust-panel,
.trust-panel__list {
  display: grid;
  gap: var(--space-4);
}

.app-shell {
  max-width: 98rem;
  margin: 0 auto;
  padding: var(--space-6);
  gap: var(--space-5);
}

.hero-stage,
.overview-stage,
.tool-stage__reports {
  display: grid;
  gap: var(--space-4);
}

.hero-stage {
  grid-template-columns: minmax(0, 0.98fr) minmax(0, 1.02fr);
  align-items: stretch;
}

.overview-stage {
  grid-template-columns: minmax(0, 1.34fr) minmax(20rem, 0.74fr);
  align-items: start;
}

.overview-stage__aside {
  gap: var(--space-3);
}

.tool-stage__reports {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.status-card {
  align-content: start;
  min-height: 100%;
  gap: var(--space-5);
  background:
    radial-gradient(circle at top right, rgba(255, 157, 87, 0.14), transparent 26%),
    linear-gradient(160deg, rgba(11, 24, 36, 0.98), rgba(13, 28, 42, 0.9));
  border-color: rgba(255, 157, 87, 0.2);
}

.status-card[data-state='analyzing'],
.status-card[data-state='compressing'] {
  background:
    radial-gradient(circle at top right, rgba(120, 211, 203, 0.16), transparent 28%),
    linear-gradient(160deg, rgba(11, 24, 36, 0.98), rgba(13, 28, 42, 0.9));
}

.status-card[data-state='success'] {
  background:
    radial-gradient(circle at top right, rgba(73, 195, 145, 0.18), transparent 28%),
    linear-gradient(160deg, rgba(11, 24, 36, 0.98), rgba(13, 28, 42, 0.9));
}

.status-card[data-state='error'] {
  background:
    radial-gradient(circle at top right, rgba(255, 126, 117, 0.18), transparent 28%),
    linear-gradient(160deg, rgba(11, 24, 36, 0.98), rgba(13, 28, 42, 0.9));
}

.status-card__header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: var(--space-3);
}

.status-card__header > div {
  max-width: 40rem;
}

.status-card__profile {
  flex-shrink: 0;
}

.status-card h2,
.status-card__body,
.status-card__eyebrow,
.status-card__facts dt,
.status-card__facts dd,
.workflow-panel__body,
.workflow-panel__step p,
.trust-panel__body,
.trust-panel__item p {
  margin: 0;
}

.status-card__body,
.status-card__eyebrow,
.workflow-panel__body,
.workflow-panel__step p,
.trust-panel__body,
.trust-panel__item p {
  color: var(--color-ink-muted);
}

.status-card__body {
  max-width: 56ch;
  font-size: var(--font-size-3);
}

.status-card__eyebrow {
  font-size: var(--font-size-1);
}

.status-card__facts {
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--space-3);
}

.status-card__facts div {
  padding: var(--space-3);
  border-radius: var(--radius-2);
  border: 1px solid var(--color-line);
  background: linear-gradient(180deg, rgba(10, 22, 34, 0.84), rgba(18, 36, 54, 0.9));
}

.workflow-panel,
.trust-panel {
  gap: var(--space-3);
  background: rgba(9, 21, 31, 0.76);
}

.workflow-panel__step,
.trust-panel__item {
  padding: var(--space-3);
  border-radius: var(--radius-2);
  border: 1px solid rgba(194, 211, 230, 0.12);
  background: rgba(13, 28, 42, 0.72);
}

.status-card__facts dt {
  margin-bottom: var(--space-1);
  color: var(--color-ink-muted);
  font-size: var(--font-size-1);
}

.status-card__facts dd {
  line-height: 1.25;
  font-weight: 600;
}

.status-card__facts div:first-child dd {
  overflow-wrap: anywhere;
}

.workflow-panel__list {
  display: grid;
  gap: var(--space-3);
  margin: 0;
  padding: 0;
  list-style: none;
}

.workflow-panel__step {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: var(--space-3);
  align-items: start;
  transition:
    border-color var(--transition-fast),
    background-color var(--transition-fast);
}

.workflow-panel__step--active {
  border-color: rgba(255, 157, 87, 0.24);
  background: linear-gradient(135deg, rgba(255, 157, 87, 0.12), rgba(14, 29, 44, 0.88));
}

.workflow-panel__number {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 2.5rem;
  height: 2.5rem;
  border-radius: 50%;
  border: 1px solid var(--color-line-strong);
  background: rgba(6, 14, 22, 0.9);
  color: var(--color-accent-strong);
  font-family: var(--font-mono);
  font-size: var(--font-size-1);
  font-weight: 500;
}

.workflow-panel__step strong,
.trust-panel__item strong {
  display: block;
  margin-bottom: var(--space-1);
}

.trust-panel__list {
  grid-template-columns: repeat(1, minmax(0, 1fr));
  gap: var(--space-2);
}

.trust-panel__item {
  align-content: start;
  min-height: 100%;
}

@media (max-width: 80rem) {
  .hero-stage,
  .overview-stage,
  .tool-stage__reports {
    grid-template-columns: repeat(1, minmax(0, 1fr));
  }

  .overview-stage__aside {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .status-card__facts {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

@media (max-width: 62rem) {
  .overview-stage__aside {
    grid-template-columns: repeat(1, minmax(0, 1fr));
  }

  .status-card__header {
    display: grid;
  }
}

@media (max-width: 48rem) {
  .app-shell {
    padding: var(--space-4);
  }

  .status-card__facts,
  .workflow-panel__step {
    grid-template-columns: repeat(1, minmax(0, 1fr));
  }

  .workflow-panel__step {
    gap: var(--space-2);
  }
}
</style>
