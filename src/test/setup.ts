import "@testing-library/jest-dom/vitest";
import { afterEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";

// Tauri 다리 — 여기엔 없다. 기본은 «실패»다: 목록을 읽는 컴포넌트들은
// catch에서 빈 목록으로 가고, 테스트가 필요하면 mockResolvedValueOnce로 준다.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => {
    throw new Error("Tauri 밖");
  }),
  convertFileSrc: (p: string) => p,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => null),
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "0.0.0-test"),
}));

// jsdom에 없는 것들
class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= RO as unknown as typeof ResizeObserver;
globalThis.requestAnimationFrame ??= (cb) =>
  setTimeout(() => cb(performance.now()), 16) as unknown as number;
globalThis.cancelAnimationFrame ??= (id) => clearTimeout(id);

afterEach(cleanup);
