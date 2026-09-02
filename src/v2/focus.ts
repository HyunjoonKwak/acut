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
