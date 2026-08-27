import { useCallback, useRef, useState } from "react";
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

  const request = useCallback<AskFn>((a) => {
    return new Promise<boolean>((resolve) => {
      // 앞의 물음이 아직 떠 있으면 그건 취소로 닫는다. 두 개가 겹치면
      // 어느 쪽에 답한 건지 알 수 없다.
      answer.current?.(false);
      answer.current = resolve;
      setAsk(a);
    });
  }, []);

  const close = useCallback((ok: boolean) => {
    answer.current?.(ok);
    answer.current = null;
    setAsk(null);
  }, []);

  return (
    <ConfirmCtx.Provider value={request}>
      {children}
      {ask && (
        <div
          className="fixed inset-0 z-[100] flex items-center justify-center bg-black/50"
          onPointerDown={() => close(false)}
        >
          <div
            role="dialog"
            aria-modal="true"
            onPointerDown={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              if (e.key === "Escape") close(false);
              if (e.key === "Enter") close(true);
            }}
            className="w-[380px] max-w-[90vw] rounded-xl bg-chrome ring-1 ring-line-strong shadow-2xl p-5"
          >
            <div className="text-[14px] font-semibold text-fg">{ask.title}</div>
            {ask.lines && ask.lines.length > 0 && (
              <ul className="mt-3 space-y-1">
                {ask.lines.map((l, i) => (
                  <li
                    key={i}
                    className="text-[12.5px] text-fg-dim leading-relaxed"
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
