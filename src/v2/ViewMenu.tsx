import { useEffect, useRef, useState } from "react";
import { SCALINGS, STYLES, type GridStyle, type Scaling } from "./gridStyle";

/** 보기 방식 — 격자 모양과 사진을 칸에 어떻게 담을지 */
export default function ViewMenu({
  style,
  scaling,
  onStyle,
  onScaling,
  filmstrip,
  onFilmstrip,
}: {
  style: GridStyle;
  scaling: Scaling;
  onStyle: (s: GridStyle) => void;
  onScaling: (s: Scaling) => void;
  filmstrip: boolean;
  onFilmstrip: (v: boolean) => void;
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

  const icon = style === "card" ? "▤" : style === "tile" ? "▦" : "▥";

  return (
    <div ref={box} className="relative">
      <button
        onClick={() => setOpen((o) => !o)}
        title="보기 방식"
        className="h-7 px-2.5 rounded-md text-[12.5px] text-[#A3B2B4] ring-1 ring-[#333C3F]"
      >
        {icon}
      </button>
      {open && (
        <div className="absolute right-0 top-8 z-30 min-w-[130px] bg-[#232A2C] rounded-md ring-1 ring-[#3B4649] shadow-xl py-1">
          {STYLES.map((s) => (
            <button
              key={s.v}
              onClick={() => {
                onStyle(s.v);
                setOpen(false);
              }}
              className={`block w-full text-left px-3 py-1.5 text-[12.5px] hover:bg-[#2E3739] ${
                s.v === style ? "text-[#49B8B4]" : "text-[#A3B2B4]"
              }`}
            >
              {s.label}
            </button>
          ))}
          {/* 양쪽 맞춤은 사진 비를 지키므로 담는 방식이 의미 없다 */}
          {style !== "justified" && (
            <>
              <div className="h-px bg-[#333C3F] my-1" />
              {SCALINGS.map((s) => (
                <button
                  key={s.v}
                  onClick={() => {
                    onScaling(s.v);
                    setOpen(false);
                  }}
                  className={`block w-full text-left px-3 py-1.5 text-[12.5px] hover:bg-[#2E3739] ${
                    s.v === scaling ? "text-[#49B8B4]" : "text-[#8D9A9C]"
                  }`}
                >
                  {s.label}
                </button>
              ))}
            </>
          )}
          <div className="h-px bg-[#333C3F] my-1" />
          <button
            onClick={() => {
              onFilmstrip(!filmstrip);
              setOpen(false);
            }}
            className={`block w-full text-left px-3 py-1.5 text-[12.5px] hover:bg-[#2E3739] ${
              filmstrip ? "text-[#49B8B4]" : "text-[#8D9A9C]"
            }`}
          >
            필름스트립
          </button>
        </div>
      )}
    </div>
  );
}
