// Early boot — runs as a blocking classic script before first paint.
// Kept external (not inline) because the production CSP is
// default-src 'self' with no 'unsafe-inline' for scripts.
// 早期启动 —— 以阻塞式经典脚本在首屏绘制前执行。
// 保持外部文件是因为生产 CSP 为 default-src 'self'，脚本不允许内联。

// Apply theme before first paint to avoid flash.
(function () {
  var stored = null;
  try { stored = localStorage.getItem('pdf-compressor-theme'); } catch (e) {}
  var theme = stored || 'system';
  if (theme === 'system') {
    theme = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }
  document.documentElement.setAttribute('data-theme', theme);
  // CSSOM style mutation is not subject to style-src — safe under the CSP.
  document.documentElement.style.colorScheme = theme;
})();

// Localize document chrome (lang, title, loading text) before first paint,
// mirroring detectInitialLocale() in src/i18n/index.ts.
(function () {
  var stored = null;
  try { stored = localStorage.getItem('pdf-compressor-locale'); } catch (e) {}
  var browserLocale = '';
  try {
    browserLocale = (navigator.languages && navigator.languages[0]) || navigator.language || '';
  } catch (e) {}
  var isZh = (stored || browserLocale).toLowerCase().indexOf('zh') === 0;
  if (isZh) {
    document.documentElement.lang = 'zh-CN';
    document.title = 'PDF 压缩器';
    document.documentElement.style.setProperty('--app-loading-text', '正在加载 PDF 压缩器…');
  }
})();
