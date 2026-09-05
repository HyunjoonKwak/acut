import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Btn } from "./ui";
import { ConfirmCtx, type Ask, type AskFn } from "./confirmContext";

/**
 * 되돌릴 수 없는 일을 하기 전에 묻는 자리.
 *
 * `window.confirm`을 쓰다 그만뒀다. 생김새가 앱과 따로 놀고, 무엇이
 * 사라지는지 조목조목 보여 줄 수가 없고, 「지우기」 버튼을 위험하게
 * 물들일 수도 없다. 무엇보다 웹뷰를 통째로 멈춰 세운다.
 *
 * 한 번에 하나만 뜬다. 물음이 겹칠 일이 없고, 겹치면 어느 쪽에 답한 건지
 * 알 수 없다.
 */
export function ConfirmProvider({ children }: { children: React.ReactNode }) {
  const [ask, setAsk] = useState<Ask | null>(null);
  const answer = useRef<((ok: boolean) => void) | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);
  const titleId = useId();
  const descriptionId = useId();

  const request = useCallback<AskFn>((a) => {
    return new Promise<boolean>((resolve) => {
      // 앞의 물음이 아직 떠 있으면 그건 취소로 닫는다. 두 개가 겹치면
      // 어느 쪽에 답한 건지 알 수 없다.
      answer.current?.(false);
      answer.current = resolve;
      // autoFocus가 확인 단추로 옮기기 전에 기억해야 한다. effect에서 잡으면
      // 이미 확인 단추가 activeElement라 닫힌 뒤 사라진 노드에 초점을 주게 된다.
      returnFocus.current = document.activeElement as HTMLElement | null;
      setAsk(a);
    });
  }, []);

  const close = useCallback((ok: boolean) => {
    answer.current?.(ok);
    answer.current = null;
    setAsk(null);
  }, []);

  useEffect(() => {
    if (!ask) return;
    return () => {
      requestAnimationFrame(() => returnFocus.current?.focus());
    };
  }, [ask]);

  return (
    <ConfirmCtx.Provider value={request}>
      {children}
      {ask && (
        <div
          className="fixed inset-0 z-[100] flex items-center justify-center bg-black/50"
          onPointerDown={() => close(false)}
        >
          <div
            ref={box}
            role="dialog"
            aria-modal="true"
            aria-labelledby={titleId}
            aria-describedby={ask.lines?.length ? descriptionId : undefined}
            onPointerDown={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              // 확인창 뒤의 격자 단축키(P/X/별점)가 함께 실행되면 안 된다.
              e.stopPropagation();
              // Enter 는 일부러 듣지 않는다 — 초점 있는 단추가 브라우저 기본으로 눌린다
              // (확인 단추가 첫 초점, Tab 으로 취소로 옮기면 취소). «항상 확인»은
              // 영구 삭제 같은 물음에서 실수 여지가 커서 뺐다 (2026-09-05 결정).
              if (e.key === "Escape") close(false);
              if (e.key !== "Tab") return;
              const focusable = Array.from(
                box.current?.querySelectorAll<HTMLElement>(
                  "button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled)",
                ) ?? [],
              );
              if (focusable.length === 0) return;
              const first = focusable[0];
              const last = focusable[focusable.length - 1];
              if (e.shiftKey && document.activeElement === first) {
                e.preventDefault();
                last.focus();
              } else if (!e.shiftKey && document.activeElement === last) {
                e.preventDefault();
                first.focus();
              }
            }}
            className="w-[380px] max-w-[90vw] rounded-xl bg-chrome ring-1 ring-line-strong shadow-2xl p-5"
          >
            <div id={titleId} className="text-[15px] font-semibold text-fg">
              {ask.title}
            </div>
            {ask.lines && ask.lines.length > 0 && (
              <ul id={descriptionId} className="mt-3 space-y-1">
                {ask.lines.map((l, i) => (
                  <li
                    key={i}
                    className="text-[13.5px] text-fg-dim leading-relaxed"
                  >
                    {l}
                  </li>
                ))}
              </ul>
            )}
            <div className="mt-5 flex justify-end gap-2">
              <Btn onClick={() => close(false)}>취소</Btn>
              <Btn
                tone={ask.danger ? "drop" : "accent"}
                autoFocus
                onClick={() => close(true)}
              >
                {ask.confirmLabel ?? "확인"}
              </Btn>
            </div>
          </div>
        </div>
      )}
    </ConfirmCtx.Provider>
  );
}
