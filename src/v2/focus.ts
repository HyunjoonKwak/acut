import { useEffect, useRef, type RefObject } from "react";

const FOCUSABLE =
  "button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), a[href], [tabindex]:not([tabindex='-1'])";

/**
 * 메뉴가 닫힐 때 다른 컨트롤이나 새 대화상자가 이미 초점을 가져갔다면
 * 트리거로 되돌리지 않는다. 메뉴 항목이 확인창을 연 직후의 초점 탈취와,
 * 열린 메뉴 밖의 검색 입력을 클릭했을 때의 역행을 막는다.
 */
export function restoreFocusIfUnclaimed(target: HTMLElement | null) {
  const active = document.activeElement;
  const dialog = document.querySelector("[role='dialog'][aria-modal='true']");
  if (!dialog && (!active || active === document.body)) target?.focus();
}

/**
 * 모달의 첫 초점·Tab 순환·Esc·닫힌 뒤 초점 복원을 한 규칙으로 묶는다.
 * 각 대화상자가 Escape만 따로 듣고 Tab을 뒤 화면으로 흘리던 차이를 없앤다.
 */
export function useModalFocus(
  dialogRef: RefObject<HTMLElement | null>,
  onClose: () => void,
) {
  const closeRef = useRef(onClose);
  useEffect(() => {
    closeRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const frame = requestAnimationFrame(() => {
      const dialog = dialogRef.current;
      const first = dialog?.querySelector<HTMLElement>(FOCUSABLE);
      (first ?? dialog)?.focus();
    });
    const key = (event: KeyboardEvent) => {
      const dialog = dialogRef.current;
      if (!dialog) return;
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        closeRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      // Selector 목록의 순서가 아니라 실제 DOM 순서로 순환해야 한다.
      // (일부 DOM 구현은 쉼표 선택자 결과를 선택자별로 묶어 돌려준다.)
      const focusable = Array.from(dialog.querySelectorAll<HTMLElement>("*")).filter(
        (element) => element.matches(FOCUSABLE),
      );
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
      event.stopPropagation();
    };
    window.addEventListener("keydown", key, true);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("keydown", key, true);
      requestAnimationFrame(() => restoreFocusIfUnclaimed(previous));
    };
  }, [dialogRef]);
}
