import { useEffect, useMemo, useRef } from "react";

export type MenuItem =
  | {
      kind: "item";
      label: string;
      hint?: string;
      danger?: boolean;
      /** 물음 상자를 띄우는 항목이 있어 기다릴 수 있어야 한다 */
      run: () => void | Promise<void>;
    }
  | { kind: "sep" };

/** 한 줄·구분선·안팎 여백의 높이 (px). 클래스와 같이 바꿔야 한다. */
const MENU_W = 180;
const ROW_H = 30;
const SEP_H = 9;
const PAD_V = 4;

export type MenuAt = { x: number; y: number } | null;

/**
 * 우클릭 메뉴.
 *
 * 화면 밖으로 나가지 않게 접었다 편다 — 오른쪽 끝이나 아래쪽 사진을 우클릭하면
 * 메뉴가 잘려서 아무것도 못 누르게 된다.
 */
export default function ContextMenu({
  at,
  items,
  onClose,
}: {
  at: MenuAt;
  items: MenuItem[];
  onClose: () => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);

  /// 화면 밖으로 나가지 않게 자리를 잡는다.
  ///
  /// 그려 놓고 재서 옮기지 않는다 — 한 프레임 보였다 튀고, 그걸 감추려고
  /// visibility를 만지는 것이 더 번거롭다. 줄 높이는 정해져 있으니 셈으로
  /// 충분하다.
  const pos = useMemo(() => {
    if (!at) return null;
    const h =
      items.reduce((a, it) => a + (it.kind === "sep" ? SEP_H : ROW_H), 0) +
      PAD_V * 2;
    return {
      left: Math.min(at.x, window.innerWidth - MENU_W - 8),
      top: Math.min(at.y, window.innerHeight - h - 8),
    };
  }, [at, items]);

  useEffect(() => {
    if (!at) return;
    returnFocus.current = document.activeElement as HTMLElement | null;
    const frame = requestAnimationFrame(() =>
      box.current?.querySelector<HTMLElement>("[role='menuitem']")?.focus(),
    );
    const away = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) onClose();
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    window.addEventListener("scroll", onClose, true);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("mousedown", away);
      window.removeEventListener("keydown", esc);
      window.removeEventListener("scroll", onClose, true);
      requestAnimationFrame(() => returnFocus.current?.focus());
    };
  }, [at, onClose]);

  if (!at) return null;

  return (
    <div
      ref={box}
      role="menu"
      aria-label="사진 작업"
      onKeyDown={(e) => {
        if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return;
        const rows = Array.from(
          box.current?.querySelectorAll<HTMLElement>("[role='menuitem']") ?? [],
        );
        if (rows.length === 0) return;
        e.preventDefault();
        e.stopPropagation();
        const i = rows.indexOf(document.activeElement as HTMLElement);
        const next =
          e.key === "Home"
            ? 0
            : e.key === "End"
              ? rows.length - 1
              : e.key === "ArrowDown"
                ? (i + 1 + rows.length) % rows.length
                : (i - 1 + rows.length) % rows.length;
        rows[next]?.focus();
      }}
      className="fixed z-50 min-w-[180px] bg-raised rounded-md ring-1 ring-line-strong shadow-2xl py-1"
      style={{ left: pos?.left ?? at.x, top: pos?.top ?? at.y }}
    >
      {items.map((it, i) =>
        it.kind === "sep" ? (
          <div key={i} role="separator" className="h-px bg-line-strong my-1" />
        ) : (
          <button
            key={i}
            role="menuitem"
            onClick={() => {
              it.run();
              onClose();
            }}
            className={`flex w-full items-center gap-3 px-3 py-1.5 text-[13.5px] hover:bg-hover ${
              it.danger ? "text-drop" : "text-fg"
            }`}
          >
            <span className="flex-1 text-left whitespace-nowrap">
              {it.label}
            </span>
            {it.hint && (
              <span className="text-[11.5px] font-mono text-fg-mute">
                {it.hint}
              </span>
            )}
          </button>
        ),
      )}
    </div>
  );
}
