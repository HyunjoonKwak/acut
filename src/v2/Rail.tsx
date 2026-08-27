/**
 * 왼쪽 아이콘 레일 — 사이드바가 무엇을 보여줄지 고른다.
 *
 * Lap의 Home.vue와 같은 구조다. 사이드바를 여러 갈래(라이브러리·폴더·날짜·
 * 카메라…)로 쓰려면 그 갈래를 고르는 자리가 따로 있어야 한다. 갈래를 다시
 * 누르면 패널이 접혀 사진이 넓어진다.
 */

export type Source =
  "library" | "folder" | "date" | "camera" | "rating" | "trash";

/** Lap의 레일 순서를 따른다: 모아 보기 → 위치 → 시간 → 장비 → 판정 → 버린 것 */
export const SOURCES: { v: Source; icon: string; label: string }[] = [
  { v: "library", icon: "▦", label: "모든 사진" },
  { v: "folder", icon: "🗀", label: "폴더" },
  { v: "date", icon: "🗓", label: "날짜" },
  { v: "camera", icon: "📷", label: "카메라" },
  { v: "rating", icon: "★", label: "평점" },
  { v: "trash", icon: "🗑", label: "휴지통" },
];

export default function Rail({
  value,
  open,
  onPick,
  trashCount,
}: {
  value: Source;
  /** 패널이 펴져 있는가 */
  open: boolean;
  onPick: (s: Source) => void;
  trashCount: number;
}) {
  return (
    <div className="w-14 shrink-0 flex flex-col items-center gap-0.5 py-2 bg-rail border-r border-line">
      {SOURCES.map((s) => {
        const on = value === s.v && open;
        return (
          <button
            key={s.v}
            onClick={() => onPick(s.v)}
            title={s.label}
            className={`relative w-12 py-1.5 rounded-lg flex flex-col items-center gap-0.5 transition-colors ${
              on
                ? "bg-raised text-accent"
                : "text-fg-mute hover:text-fg hover:bg-chrome"
            }`}
          >
            <span className="text-[15px] leading-none">{s.icon}</span>
            <span className="text-[9px] leading-none">{s.label}</span>
            {s.v === "trash" && trashCount > 0 && (
              <span className="absolute top-0 right-1 min-w-[14px] h-[14px] px-0.5 rounded-full bg-drop text-drop-fg text-[9px] font-bold flex items-center justify-center tabular-nums">
                {trashCount > 99 ? "99+" : trashCount}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}
