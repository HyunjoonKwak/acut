import { useCallback, useRef, useState } from "react";
import { Btn } from "./ui";
import { badName } from "./fileName";
import { useModalFocus } from "./focus";

/**
 * 이름 바꾸기 — 확장자 앞부분만 고르고 뜬다. 보통 바꾸는 건 그쪽이다.
 * 같은 이름이 있으면 바꾸지 않고 알린다 (백엔드가 거절한다).
 */
export default function RenameDialog({
  name,
  onSubmit,
  onClose,
}: {
  name: string;
  /** 새 이름. 실패하면 던진다 — 상자가 그 말을 보여 준다. */
  onSubmit: (next: string) => Promise<void>;
  onClose: () => void;
}) {
  const [value, setValue] = useState(name);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const problem = badName(value);
  const unchanged = value.trim() === name;

  const submit = async () => {
    if (problem || unchanged) return;
    setBusy(true);
    setErr(null);
    try {
      await onSubmit(value.trim());
      onClose();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  // 확장자 앞까지만 고른 채로 뜬다. 콜백 ref는 **안정적이어야** 한다 —
  // 인라인이면 렌더마다 다시 불려 글자를 칠 때마다 선택이 되돌아간다.
  const selectStem = useCallback(
    (el: HTMLInputElement | null) => {
      if (!el) return;
      const dot = name.lastIndexOf(".");
      el.focus();
      el.setSelectionRange(0, dot > 0 ? dot : name.length);
    },
    [name],
  );

  const dialogRef = useRef<HTMLDivElement>(null);
  useModalFocus(dialogRef, onClose, { locked: busy });

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/50"
      onPointerDown={onClose}
    >
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label="이름 바꾸기"
        onPointerDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") submit();
        }}
        className="w-[380px] max-w-[90vw] rounded-xl bg-chrome ring-1 ring-line-strong shadow-2xl p-5"
      >
        <div className="text-[15px] font-semibold text-fg">이름 바꾸기</div>
        <input
          ref={selectStem}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          aria-label="새 이름"
          className="mt-3 w-full h-control px-2 rounded-md bg-canvas text-[14px] text-fg
            outline-none ring-1 ring-line focus:ring-accent"
        />
        {(problem && value !== name) || err ? (
          <div className="mt-2 text-[13px] text-drop">{err ?? problem}</div>
        ) : (
          <div className="mt-2 text-[13px] text-fg-mute">
            같은 이름이 있으면 바꾸지 않습니다.
          </div>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <Btn onClick={onClose}>취소</Btn>
          <Btn
            tone="accent"
            disabled={busy || !!problem || unchanged}
            onClick={submit}
          >
            바꾸기
          </Btn>
        </div>
      </div>
    </div>
  );
}
