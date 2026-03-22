<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type { AppLocale } from '../i18n'

const props = defineProps<{
  fileName: string
  nativeAvailable: boolean
  locale: AppLocale
  locales: readonly AppLocale[]
}>()

const emit = defineEmits<{
  'update:locale': [value: AppLocale]
}>()

const { t } = useI18n()

const localeOptions = computed(() =>
  props.locales.map((locale) => ({
    value: locale,
    label: t(`locale.${locale}`),
  })),
)

const headerStats = computed(() => [
  {
    label: t('header.stats.flow'),
    value: t('header.stats.flowValue'),
  },
  {
    label: t('header.stats.runtime'),
    value: props.nativeAvailable ? t('header.nativeReady') : t('header.previewMode'),
  },
  {
    label: t('header.stats.current'),
    value: props.fileName || t('header.emptyFile'),
  },
])
</script>

<template>
  <header class="app-header panel-surface">
    <div class="app-header__topbar">
      <div class="app-header__brand">
        <span class="app-header__mark">PX</span>
        <div>
          <p class="app-header__eyebrow">{{ t('header.eyebrow') }}</p>
          <strong class="app-header__microcopy">{{ t('header.localOnly') }}</strong>
        </div>
      </div>

      <label class="app-header__locale" for="locale-select">
        <span class="app-header__locale-label">{{ t('locale.label') }}</span>
        <select
          id="locale-select"
          class="app-select"
          :value="props.locale"
          @change="emit('update:locale', ($event.target as HTMLSelectElement).value as AppLocale)"
        >
          <option v-for="option in localeOptions" :key="option.value" :value="option.value">
            {{ option.label }}
          </option>
        </select>
      </label>
    </div>

    <div class="app-header__hero">
      <div class="app-header__copy">
        <h1>{{ t('header.title') }}</h1>
        <p class="app-header__body">{{ t('header.body') }}</p>

        <div class="app-header__chips">
          <span class="app-chip app-chip--accent app-header__chip">{{ props.nativeAvailable ? t('header.nativeReady') : t('header.previewMode') }}</span>
          <span class="app-chip app-header__chip">{{ t('header.stats.flowValue') }}</span>
          <span class="app-chip app-header__chip app-header__chip--file" :title="props.fileName || t('header.emptyFile')">{{ props.fileName || t('header.emptyFile') }}</span>
        </div>
      </div>

      <dl class="app-header__stats">
        <div v-for="stat in headerStats" :key="stat.label">
          <dt>{{ stat.label }}</dt>
          <dd>{{ stat.value }}</dd>
        </div>
      </dl>
    </div>
  </header>
</template>

<style scoped>
.app-header,
.app-header__copy,
.app-header__locale,
.app-header__chips,
.app-header__stats {
  display: grid;
  gap: var(--space-4);
}

.app-header {
  align-content: space-between;
  min-height: 100%;
  gap: var(--space-5);
}

.app-header__topbar,
.app-header__brand,
.app-header__hero {
  display: flex;
  gap: var(--space-4);
}

.app-header__topbar {
  justify-content: space-between;
  align-items: center;
}

.app-header__brand {
  align-items: center;
}

.app-header__mark {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 3.5rem;
  height: 3.5rem;
  border-radius: 1.1rem;
  background:
    radial-gradient(circle at top left, rgba(120, 211, 203, 0.34), transparent 42%),
    linear-gradient(135deg, rgba(255, 157, 87, 0.22), rgba(8, 18, 28, 0.95));
  border: 1px solid rgba(255, 157, 87, 0.22);
  color: var(--color-ink-strong);
  font-family: var(--font-display);
  font-size: 1.25rem;
}

.app-header__eyebrow,
.app-header__body,
.app-header__locale-label,
.app-header__stats dt,
.app-header__microcopy {
  margin: 0;
}

.app-header__eyebrow,
.app-header__microcopy,
.app-header__stats dt {
  font-size: var(--font-size-0);
  letter-spacing: 0.18em;
  text-transform: uppercase;
}

.app-header__eyebrow,
.app-header__stats dt {
  color: var(--color-accent-secondary);
}

.app-header__microcopy,
.app-header__locale-label,
.app-header__body {
  color: var(--color-ink-muted);
}

.app-header__microcopy {
  font-weight: 600;
}

.app-header__locale {
  justify-items: end;
}

.app-header__hero {
  align-items: end;
  justify-content: space-between;
}

.app-header__copy {
  max-width: 36rem;
}

h1 {
  margin: 0;
  max-width: 11ch;
}

.app-header__body {
  max-width: 48ch;
  font-size: var(--font-size-3);
}

.app-header__chips {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  align-items: center;
  max-width: 32rem;
}

.app-header__chip {
  min-height: 2rem;
  padding-inline: var(--space-2);
}

.app-header__chip--file {
  max-width: min(100%, 17rem);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.app-header__stats {
  grid-template-columns: repeat(3, minmax(0, 1fr));
  min-width: min(100%, 24rem);
  align-self: stretch;
  gap: var(--space-2);
}

.app-header__stats div {
  display: grid;
  gap: var(--space-1);
  align-content: start;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-2);
  border: 1px solid var(--color-line);
  background: linear-gradient(180deg, rgba(7, 17, 26, 0.34), rgba(18, 36, 54, 0.62));
}

.app-header__stats dd {
  margin: 0;
  color: var(--color-ink-strong);
  font-weight: 600;
  line-height: 1.3;
}

.app-header__stats div:last-child dd {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

@media (max-width: 80rem) {
  .app-header__hero {
    flex-direction: column;
    align-items: flex-start;
  }

  .app-header__stats {
    width: 100%;
  }
}

@media (max-width: 62rem) {
  .app-header__topbar,
  .app-header__hero {
    flex-direction: column;
    align-items: flex-start;
  }

  .app-header__locale {
    justify-items: start;
  }

  .app-header__stats {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .app-header__chip--file {
    max-width: 100%;
    flex-basis: 100%;
  }
}

@media (max-width: 48rem) {
  .app-header__topbar {
    align-items: stretch;
  }

  .app-header__brand {
    gap: var(--space-3);
  }

  .app-header__stats {
    grid-template-columns: repeat(1, minmax(0, 1fr));
  }
}
</style>
