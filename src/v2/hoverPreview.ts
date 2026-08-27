/**
 * 한 번에 하나만 미리보기한다.
 *
 * 그리드를 빠르게 훑으면 지나간 타일들이 전부 재생을 붙들고 있게 된다.
 * 400MB짜리 영상 여러 개가 동시에 돌면 앱이 멈춘다. 그래서 새 미리보기가
 * 시작될 때 앞의 것을 끈다. (Lap의 hoverPreview.ts와 같은 방식)
 */
/** 타일 하나를 알아보는 열쇠. 타일이 사는 동안 같은 객체여야 한다. */
export type PreviewKey = object;

let active: { key: PreviewKey; stop: () => void } | null = null;

/**
 * 이 타일이 미리보기를 잡는다. 앞의 것은 꺼진다.
 *
 * 열쇠와 끄는 함수를 따로 받는 이유: 끄는 함수가 자기 자신을 열쇠로 쓰면
 * 함수 안에서 자기 이름을 불러야 해서 «선언 전 참조»가 된다.
 */
export function claimHoverPreview(key: PreviewKey, stop: () => void) {
  if (active?.key === key) return;
  active?.stop();
  active = { key, stop };
}

export function releaseHoverPreview(key: PreviewKey) {
  if (active?.key === key) active = null;
}

/** 마우스를 올린 뒤 재생까지 기다리는 시간. 스쳐 지나갈 때는 재생하지 않는다. */
export const HOVER_DELAY_MS = 400;
