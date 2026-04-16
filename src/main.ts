import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import { i18n } from './i18n'
import { notifyAppReady } from './lib/tauri'

const app = createApp(App)
app.use(i18n)
app.mount('#app')

void notifyAppReady()
