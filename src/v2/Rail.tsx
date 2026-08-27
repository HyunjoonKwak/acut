import { FOOT, SOURCES, type Entry, type Source } from "./railItems";

/**
 * 왼쪽 아이콘 레일 — 사이드바가 무엇을 보여줄지 고른다.
 *
 * Lap의 Home.vue와 같은 구성이다. 갈래를 다시 누르면 패널이 접혀 사진이
 * 넓어진다. 휴지통과 설정은 성격이 달라 맨 아래에 따로 둔다.
 */
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
  const item = (s: Entry) => {
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
  };

  return (
    <div className="w-14 shrink-0 flex flex-col items-center gap-0.5 py-2 bg-rail border-r border-line">
      {SOURCES.map(item)}
      <div className="flex-1" />
      {FOOT.map(item)}
    </div>
  );
}
