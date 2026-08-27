import { useEffect, useRef, useState } from "react";

/** 백엔드 `db::query::SortBy`와 이름이 같아야 한다 */
export type SortBy =
  | "taken_at"
  | "created_at"
  | "modified_at"
  | "name"
  | "size"
  | "pixels"
  | "duration";

export type Sort = { by: SortBy; desc: boolean };

export const DEFAULT_SORT: Sort = { by: "taken_at", desc: true };

/** Lap의 정렬 목록과 같다 */
const ITEMS: { by: SortBy; label: string }[] = [
  { by: "taken_at", label: "촬영일" },
  { by: "created_at", label: "생성일" },
  { by: "modified_at", label: "수정일" },
  { by: "name", label: "이름" },
  { by: "size", label: "크기" },
  { by: "pixels", label: "픽셀 크기" },
  { by: "duration", label: "재생시간" },
];

export const sortLabel = (s: Sort) =>
  ITEMS.find((i) => i.by === s.by)?.label ?? "정렬";

export default function SortMenu({
  value,
  onChange,
}: {
  value: Sort;
  onChange: (s: Sort) => void;
}) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  // 바깥을 누르면 닫힌다. 메뉴가 열린 채 남으면 그리드를 가린다.
  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", away);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);

  return (
    <div ref={box} className="relative">
      <button
        onClick={() => setOpen((o) => !o)}
        title="정렬 기준"
        className="h-7 px-2.5 rounded-md text-[12.5px] text-[#A3B2B4] ring-1 ring-[#333C3F] flex items-center gap-1"
      >
        <span className="text-[#6D7B7E]">{value.desc ? "↓" : "↑"}</span>
        {sortLabel(value)}
      </button>

      {open && (
        <div className="absolute left-0 top-8 z-30 min-w-[140px] bg-[#232A2C] rounded-md ring-1 ring-[#3B4649] shadow-xl py-1">
          {ITEMS.map((i) => (
            <button
              key={i.by}
              onClick={() => {
                // 같은 기준을 다시 누르면 방향이 바뀐다
                onChange(
                  i.by === value.by
                    ? { by: i.by, desc: !value.desc }
                    : { by: i.by, desc: true },
                );
                setOpen(false);
              }}
              className={`block w-full text-left px-3 py-1.5 text-[12.5px] hover:bg-[#2E3739] ${
                i.by === value.by ? "text-[#49B8B4]" : "text-[#A3B2B4]"
              }`}
            >
              {i.by === value.by && (
                <span className="mr-1">{value.desc ? "↓" : "↑"}</span>
              )}
              {i.label}
            </button>
          ))}
          <div className="h-px bg-[#333C3F] my-1" />
          <button
            onClick={() => {
              onChange({ ...value, desc: !value.desc });
              setOpen(false);
            }}
            className="block w-full text-left px-3 py-1.5 text-[12.5px] text-[#8D9A9C] hover:bg-[#2E3739]"
          >
            {value.desc ? "↑ 오름차순으로" : "↓ 내림차순으로"}
          </button>
        </div>
      )}
    </div>
  );
}
