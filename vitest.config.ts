import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

/**
 * 컴포넌트 테스트 — jsdom에서 그려 보고 눌러 본다.
 *
 * 순수 셈(레이아웃·스크롤바·필터·스토어)은 그대로 node:test다 (`npm run
 * test:logic`). 여기는 «그려지는가, 눌리는가»만 본다. Tauri는 setup에서
 * 통째로 흉내 낸다 — 백엔드 없이 돈다.
 */
export default defineConfig({
  plugins: [react()],
  define: { __APP_VERSION__: JSON.stringify("0.0.0-test") },
  test: {
    environment: "jsdom",
    setupFiles: ["src/test/setup.ts"],
    include: ["src/**/*.test.tsx"],
    css: false,
    restoreMocks: true,
  },
});
