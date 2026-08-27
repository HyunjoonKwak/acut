import { useEffect, useRef, useState } from "react";

export type MenuItem =
  | {
      kind: "item";
      label: string;
      hint?: string;
      danger?: boolean;
      run: () => void;
    }
  | { kind: "sep" };

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
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  useEffect(() => {
    if (!at) {
      setPos(null);
      return;
    }
    const el = box.current;
    const w = el?.offsetWidth ?? 180;
    const h = el?.offsetHeight ?? 200;
    setPos({
      left: Math.min(at.x, window.innerWidth - w - 8),
      top: Math.min(at.y, window.innerHeight - h - 8),
    });
  }, [at, items.length]);

  useEffect(() => {
    if (!at) return;
    const away = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) onClose();
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    window.addEventListener("scroll", onClose, true);
    return () => {
      window.removeEventListener("mousedown", away);
      window.removeEventListener("keydown", esc);
      window.removeEventListener("scroll", onClose, true);
    };
  }, [at, onClose]);

  if (!at) return null;

  return (
    <div
      ref={box}
      className="fixed z-50 min-w-[180px] bg-raised rounded-md ring-1 ring-line-strong shadow-2xl py-1"
      style={{
        left: pos?.left ?? at.x,
        top: pos?.top ?? at.y,
        // 자리를 재기 전에는 보이지 않게 — 깜빡이며 옮겨가는 것을 막는다
        visibility: pos ? "visible" : "hidden",
      }}
    >
      {items.map((it, i) =>
        it.kind === "sep" ? (
          <div key={i} className="h-px bg-line-strong my-1" />
        ) : (
          <button
            key={i}
            onClick={() => {
              it.run();
              onClose();
            }}
            className={`flex w-full items-center gap-3 px-3 py-1.5 text-[12.5px] hover:bg-hover ${
              it.danger ? "text-drop" : "text-fg"
            }`}
          >
            <span className="flex-1 text-left whitespace-nowrap">
              {it.label}
            </span>
            {it.hint && (
              <span className="text-[10.5px] font-mono text-fg-mute">
                {it.hint}
              </span>
            )}
          </button>
        ),
      )}
    </div>
  );
}
