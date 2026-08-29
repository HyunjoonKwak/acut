import { invoke } from "@tauri-apps/api/core";

/**
 * 화면 쪽 오류를 뒷단 로그 파일에 남긴다.
 *
 * 릴리스 앱에는 웹뷰 콘솔이 없다. 그리다 예외가 나서 창이 비어 버리면 사용자도
 * 우리도 이유를 알 길이 없었다. 여기서 남긴 줄은
 * `~/Library/Logs/com.acut.media/webview.log` 에 쌓인다.
 */

/** 한 세션에 종류별로 너무 많이 남기지 않는다 — 같은 오류가 프레임마다 나면
 *  파일이 터진다. 종류별로 세는 이유: 잡음 하나가 진짜 오류의 자리를 먹지 않게. */
const MAX_PER_LEVEL = 30;
const sent = new Map<string, number>();

export function reportError(level: string, text: string): void {
  const n = sent.get(level) ?? 0;
  if (n >= MAX_PER_LEVEL) return;
  sent.set(level, n + 1);
  invoke("frontend_log", { level, msg: text }).catch(() => {
    // 로그조차 못 남기면 할 수 있는 게 없다
  });
}

function describe(e: unknown): string {
  if (e instanceof Error) return `${e.name}: ${e.message}\n${e.stack ?? ""}`;
  if (typeof e === "string") return e;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

/** 잡히지 않은 예외와 거절된 프로미스를 전부 남긴다. 앱 시작 때 한 번 부른다. */
export function installErrorLog(): void {
  window.addEventListener("error", (ev) => {
    reportError(
      "error",
      `${ev.message} @ ${ev.filename}:${ev.lineno}:${ev.colno}\n${describe(ev.error)}`,
    );
  });
  window.addEventListener("unhandledrejection", (ev) => {
    reportError("rejection", describe(ev.reason));
  });
}
