import { useEffect, useRef, useState, type RefObject } from "react";

const FOCUSABLE =
  "button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), a[href], [tabindex]:not([tabindex='-1'])";
const MODAL = "[role='dialog'][aria-modal='true']";

/**
 * 메뉴가 닫히며 트리거로 돌려주지 못한 초점. 그 자리를 차지한 대화상자가 닫힐 때
 * 이어받는다 — 메뉴 항목은 대화상자가 뜨는 커밋에서 이미 사라지므로 대화상자
 * 스스로는 어디로 돌아갈지 모른다.
 */
let deferredReturn: HTMLElement | null = null;

/**
 * 메뉴가 닫힐 때 다른 컨트롤이나 새 대화상자가 이미 초점을 가져갔다면
 * 트리거로 되돌리지 않는다. 메뉴 항목이 확인창을 연 직후의 초점 탈취와,
 * 메뉴 밖 입력을 클릭해 닫았을 때 그 입력의 초점을 빼앗는 일을 막는다.
 * 대화상자가 가져갔다면 그 대화상자가 닫힐 때 트리거로 돌아가게 맡겨 둔다.
 */
export function restoreFocusIfUnclaimed(target: HTMLElement | null) {
  const active = document.activeElement;
  const dialog = document.querySelector(MODAL);
  if (!dialog && (!active || active === document.body)) target?.focus();
  else if (dialog && target?.isConnected) deferredReturn = target;
}

/** 지금 맨 위에 뜬 모달. App 은 나중에 뜨는 상자를 DOM 뒤쪽에 그리므로 마지막 것이다. */
function topModal(): Element | null {
  const modals = document.querySelectorAll(MODAL);
  return modals[modals.length - 1] ?? null;
}

/**
 * 대화상자 안의 초점 가능한 요소를 문서 순서로. 쉼표 선택자 결과를 선택자별로
 * 묶어 돌려주는 DOM 구현(jsdom)이 있어 `querySelectorAll` 순서를 믿지 않는다.
 */
function focusableIn(dialog: HTMLElement): HTMLElement[] {
  return Array.from(dialog.querySelectorAll<HTMLElement>(FOCUSABLE)).sort((a, b) =>
    a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1,
  );
}

/**
 * 모달의 첫 초점·Tab 순환·Esc·닫힌 뒤 초점 복원을 한 규칙으로 묶는다.
 * 각 대화상자가 Escape만 따로 듣고 Tab을 뒤 화면으로 흘리던 차이를 없앤다.
 *
 * `locked`가 참인 동안(실행 중) Esc 는 삼키되 닫지 않는다 — 언마운트되면
 * 실패 문장을 아무도 못 본다.
 */
export function useModalFocus(
  dialogRef: RefObject<HTMLElement | null>,
  onClose: () => void,
  options: { locked?: boolean } = {},
) {
  const closeRef = useRef(onClose);
  const lockedRef = useRef(options.locked === true);
  useEffect(() => {
    closeRef.current = onClose;
    lockedRef.current = options.locked === true;
  }, [onClose, options.locked]);
  // 첫 렌더 시점에 잡는다. 커밋이 끝난 뒤(effect)에는 메뉴 항목 같은 트리거가
  // 이미 사라져 activeElement 가 body 다.
  const [previous] = useState(
    () => document.activeElement as HTMLElement | null,
  );

  useEffect(() => {
    // 앞선 대화상자가 남긴 복귀 대상은 이 상자와 무관하다. 이 상자를 연 메뉴의
    // 복귀 대상은 이 effect 뒤의 rAF 에서 채워진다.
    deferredReturn = null;
    const frame = requestAnimationFrame(() => {
      const dialog = dialogRef.current;
      if (!dialog) return;
      // React 의 autoFocus 처럼 이미 안에 초점이 있으면 그대로 둔다.
      if (dialog.contains(document.activeElement)) return;
      (focusableIn(dialog)[0] ?? dialog).focus();
    });
    const key = (event: KeyboardEvent) => {
      const dialog = dialogRef.current;
      // 위에 다른 모달(가져오기·확인창)이 떠 있으면 그쪽 몫이다.
      if (!dialog || topModal() !== dialog) return;
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        if (!lockedRef.current) closeRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      event.stopPropagation();
      const focusable = focusableIn(dialog);
      if (focusable.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (!dialog.contains(active)) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      } else if (event.shiftKey && active === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", key, true);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("keydown", key, true);
      const back =
        previous && previous !== document.body && previous.isConnected
          ? previous
          : deferredReturn;
      deferredReturn = null;
      requestAnimationFrame(() => restoreFocusIfUnclaimed(back));
    };
  }, [dialogRef, previous]);
}
