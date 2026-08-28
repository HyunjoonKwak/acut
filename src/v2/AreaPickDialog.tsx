import { useEffect } from "react";
import { AREAS, type Area } from "./areaItems";
import { Btn } from "./ui";

/**
 * 라이브러리를 등록할 때 — 이 폴더는 어느 영역인가.
 *
 * 영역이 곧 흐름이라 처음에 정해야 한다. 나중에 라이브러리 「⋯」에서 바꿀 수 있다.
 */
export default function AreaPickDialog({
  path,
  onPick,
  onClose,
}: {
  path: string;
  onPick: (area: Area) => void;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-[70] bg-canvas/80 backdrop-blur-sm flex items-center justify-center p-6">
      <div className="w-[460px] max-w-full bg-chrome rounded-xl ring-1 ring-line shadow-2xl p-5">
        <div className="text-[15px] font-semibold text-fg mb-1">
          이 폴더는 어느 영역입니까?
        </div>
        <div className="text-[12px] text-fg-mute truncate mb-4" title={path}>
          {path}
        </div>
        <div className="flex flex-col gap-1.5 mb-4">
          {AREAS.map((a) => (
            <button
              key={a.v}
              onClick={() => onPick(a.v)}
              className="text-left px-3 py-2 rounded-md ring-1 ring-line hover:ring-accent hover:bg-hover"
            >
              <div className="text-[13px] text-fg font-medium">{a.label}</div>
              <div className="text-[11.5px] text-fg-mute">{a.hint}</div>
            </button>
          ))}
        </div>
        <div className="flex justify-end">
          <Btn onClick={onClose} hint="Esc">
            취소
          </Btn>
        </div>
      </div>
    </div>
  );
}
