<script setup lang="ts">
/**
 * Software-update card for the settings view — manual check, explicit
 * download confirmation, and a relaunch prompt. No silent auto-updates.
 * 设置页的软件更新卡片 — 手动检查、确认后下载安装、提示重启；不做静默更新。
 */
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import {
  checkForAppUpdate,
  downloadAndInstallAppUpdate,
  hasNativeCommands,
  relaunchApp,
  type AppUpdateInfo,
} from '../lib/tauri'
import { formatBytes } from '../utils/format'

const { t } = useI18n()

type PanelState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'upToDate'; version: string }
  | { kind: 'available'; update: AppUpdateInfo }
  | { kind: 'downloading'; downloaded: number; total?: number }
  | { kind: 'installing' }
  | { kind: 'installed' }
  | { kind: 'failed'; detail: string }

const state = ref<PanelState>({ kind: 'idle' })
const nativeAvailable = hasNativeCommands()

const canCheck = computed(
  () =>
    state.value.kind === 'idle' || state.value.kind === 'upToDate' || state.value.kind === 'failed',
)

async function runCheck() {
  if (!nativeAvailable) {
    state.value = { kind: 'failed', detail: t('updater.unavailableBody') }
    return
  }
  state.value = { kind: 'checking' }
  try {
    const result = await checkForAppUpdate()
    state.value = result.available
      ? { kind: 'available', update: result.info }
      : { kind: 'upToDate', version: result.currentVersion }
  } catch (error) {
    state.value = {
      kind: 'failed',
      detail: error instanceof Error ? error.message : String(error),
    }
  }
}

async function runDownloadAndInstall() {
  if (state.value.kind !== 'available') {
    return
  }
  state.value = { kind: 'downloading', downloaded: 0 }
  try {
    await downloadAndInstallAppUpdate((downloaded, total) => {
      state.value = { kind: 'downloading', downloaded, total }
    })
    state.value = { kind: 'installing' }
    // Windows/macOS installers apply the update and exit; the relaunch call
    // covers the Linux AppImage path and any build that keeps running.
    await relaunchApp()
    state.value = { kind: 'installed' }
  } catch (error) {
    state.value = {
      kind: 'failed',
      detail: error instanceof Error ? error.message : String(error),
    }
  }
}

const progressLabel = computed(() => {
  if (state.value.kind !== 'downloading') {
    return ''
  }
  const done = formatBytes(state.value.downloaded)
  const total = state.value.total ? formatBytes(state.value.total) : ''
  return t('updater.downloading', {
    contentLength: total ? `${done} / ${total}` : done,
  })
})
</script>

<template>
  <section class="updater-panel">
    <header class="updater-panel__header">
      <h2 class="updater-panel__title">{{ t('updater.title') }}</h2>
      <button type="button" class="btn btn-ghost" :disabled="!canCheck" @click="runCheck">
        {{ state.kind === 'checking' ? t('updater.checking') : t('updater.checkForUpdates') }}
      </button>
    </header>

    <p v-if="state.kind === 'upToDate'" class="updater-panel__note">
      {{ t('updater.upToDateBody', { version: state.version || '—' }) }}
    </p>

    <template v-else-if="state.kind === 'available'">
      <p class="updater-panel__note">
        {{ t('updater.availableBody', { version: state.update.version }) }}
      </p>
      <div class="updater-panel__actions">
        <button type="button" class="btn btn-primary" @click="runDownloadAndInstall">
          {{ t('updater.downloadAndInstall') }}
        </button>
        <button type="button" class="btn btn-ghost" @click="state = { kind: 'idle' }">
          {{ t('updater.later') }}
        </button>
      </div>
    </template>

    <p v-else-if="state.kind === 'downloading'" class="updater-panel__note">
      {{ progressLabel }}
    </p>
    <p v-else-if="state.kind === 'installing'" class="updater-panel__note">
      {{ t('updater.installing') }}
    </p>
    <template v-else-if="state.kind === 'installed'">
      <p class="updater-panel__note">{{ t('updater.installedBody') }}</p>
      <div class="updater-panel__actions">
        <button type="button" class="btn btn-primary" @click="relaunchApp()">
          {{ t('updater.restartNow') }}
        </button>
      </div>
    </template>
    <p v-else-if="state.kind === 'failed'" class="updater-panel__note updater-panel__note--error">
      {{ t('updater.failedBody', { detail: state.detail }) }}
    </p>
  </section>
</template>

<style scoped>
.updater-panel {
  display: flex;
  flex-direction: column;
  gap: var(--fd-space-10);
}

.updater-panel__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fd-space-10);
}

.updater-panel__title {
  margin: 0;
  font: var(--fd-text-subtitle);
}

.updater-panel__note {
  margin: 0;
  font: var(--fd-text-body);
  color: var(--fd-text-secondary);
}

.updater-panel__note--error {
  color: var(--fd-danger, #dc2626);
}

.updater-panel__actions {
  display: flex;
  gap: var(--fd-space-8);
}
</style>
