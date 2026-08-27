/**
 * 한 번에 하나만 미리보기한다.
 *
 * 그리드를 빠르게 훑으면 지나간 타일들이 전부 재생을 붙들고 있게 된다.
 * 400MB짜리 영상 여러 개가 동시에 돌면 앱이 멈춘다. 그래서 새 미리보기가
 * 시작될 때 앞의 것을 끈다. (Lap의 hoverPreview.ts와 같은 방식)
 */
let activeStop: (() => void) | null = null;

export function claimHoverPreview(stop: () => void) {
  if (activeStop === stop) return;
  activeStop?.();
  activeStop = stop;
}

export function releaseHoverPreview(stop: () => void) {
  if (activeStop === stop) activeStop = null;
}

/** 마우스를 올린 뒤 재생까지 기다리는 시간. 스쳐 지나갈 때는 재생하지 않는다. */
export const HOVER_DELAY_MS = 400;
