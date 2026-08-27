import { useEffect, useRef, useState } from "react";

/** 백엔드 `db::query::GroupBy`와 이름이 같아야 한다 */
export type GroupBy =
  | "none"
  | "folder"
  | "day"
  | "month"
  | "year"
  | "rating"
  | "camera"
  | "lens"
  | "file_type"
  | "culling";

const ITEMS: { by: GroupBy; label: string }[] = [
  { by: "none", label: "묶지 않음" },
  { by: "day", label: "날짜" },
  { by: "month", label: "월" },
  { by: "year", label: "연도" },
  { by: "folder", label: "폴더" },
  { by: "rating", label: "평점" },
  { by: "culling", label: "판정" },
  { by: "file_type", label: "종류" },
  { by: "camera", label: "카메라" },
  { by: "lens", label: "렌즈" },
];

export default function GroupMenu({
  value,
  onChange,
}: {
  value: GroupBy;
  onChange: (g: GroupBy) => void;
}) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);

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

  const cur = ITEMS.find((i) => i.by === value) ?? ITEMS[0];

  return (
    <div ref={box} className="relative">
      <button
        onClick={() => setOpen((o) => !o)}
        title="묶어 보기"
        className={`h-7 px-2.5 rounded-md text-[12.5px] ring-1 ring-[#333C3F] ${
          value === "none" ? "text-[#A3B2B4]" : "text-[#49B8B4]"
        }`}
      >
        ▤ {cur.label}
      </button>
      {open && (
        <div className="absolute right-0 top-8 z-30 min-w-[120px] bg-[#232A2C] rounded-md ring-1 ring-[#3B4649] shadow-xl py-1">
          {ITEMS.map((i) => (
            <button
              key={i.by}
              onClick={() => {
                onChange(i.by);
                setOpen(false);
              }}
              className={`block w-full text-left px-3 py-1.5 text-[12.5px] hover:bg-[#2E3739] ${
                i.by === value ? "text-[#49B8B4]" : "text-[#A3B2B4]"
              }`}
            >
              {i.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
