import { defineConfig } from 'vite-plus'
import vue from '@vitejs/plugin-vue'

// https://vite.dev/config/
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: 'happy-dom',
    include: ['src/**/*.spec.ts'],
  },
  fmt: {
    // 沿用项目原有风格（单引号、无分号）；工具链经 `vp -C frontend` 以本目录为
    // cwd 运行，格式化范围即 frontend/ 自身，仓库其余部分不在覆盖范围内。
    // src/lib/bindings.ts 是 tauri-specta 生成物（cargo test 会再生），不参与格式化，
    // 否则与生成器互相覆盖。
    singleQuote: true,
    semi: false,
    ignorePatterns: ['public/**', 'src/lib/bindings.ts'],
  },
  lint: {
    // public/ 是手工维护的早期引导脚本（ES5 风格，吞错的 try/catch 是刻意的）；
    // bindings.ts 为生成物。两者不参与 lint。
    ignorePatterns: ['public/**', 'src/lib/bindings.ts'],
  },
})
