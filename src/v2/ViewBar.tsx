import { useState } from "react";
import {
  IconCaption,
  IconCard,
  IconFilmstrip,
  IconJustified,
  IconMasonry,
  IconTile,
} from "./icons";
import { hasCaption, type GridStyle } from "./gridStyle";
import { next } from "./cycle";
import { usePref } from "./prefs";

/**
 * 보기 방식 — 툴바의 버튼 셋.
 *
 * 격자 모양은 **누를 때마다 다음 것으로** 바뀐다(카드 → 타일 → 양쪽 맞춤 →
 * 메이슨리). 버튼에 지금 상태의 그림과 이름을 함께 써서 열어 보지 않아도
 * 안다. 이름·크기는 카드에서만 뜨므로 카드일 때만 버튼이 보인다.
 */

type IconOf = (p: { className?: string }) => React.ReactElement;

const STYLES: { v: GridStyle; label: string; Icon: IconOf }[] = [
  { v: "card", label: "카드", Icon: IconCard },
  { v: "tile", label: "타일", Icon: IconTile },
  { v: "justified", label: "양쪽 맞춤", Icon: IconJustified },
  { v: "masonry", label: "메이슨리", Icon: IconMasonry },
];

function Tip({ children }: { children: React.ReactNode }) {
  const [tooltips] = usePref("tooltips");
  if (!tooltips) return null;
  return (
    <span
      className="absolute top-full mt-1.5 left-1/2 -translate-x-1/2 z-50 px-2 py-1 rounded-md
        bg-raised text-fg text-[11.5px] whitespace-nowrap shadow-lg ring-1 ring-line-strong
        pointer-events-none"
    >
      {children}
    </span>
  );
}

/** 돌아가며 바뀌는 버튼 — 지금 것의 그림과 이름을 쓴다 */
function Cycle<T extends { v: string; label: string; Icon: IconOf }>({
  items,
  value,
  onChange,
  what,
}: {
  items: T[];
  value: string;
  onChange: (v: T["v"]) => void;
  /** 이름표 앞에 붙는 말 — «보기» */
  what: string;
}) {
  const [hover, setHover] = useState(false);
  const cur = items.find((x) => x.v === value) ?? items[0];
  const nxt = next(items, value);
  return (
    <button
      onClick={() => onChange(nxt.v)}
      onPointerEnter={() => setHover(true)}
      onPointerLeave={() => setHover(false)}
      onFocus={() => setHover(true)}
      onBlur={() => setHover(false)}
      aria-label={`${what}: ${cur.label}`}
      className="relative h-control pl-1.5 pr-2 rounded-md inline-flex items-center gap-1.5
        bg-raised text-fg-dim hover:text-fg transition-colors"
    >
      <cur.Icon className="w-[17px] h-[17px]" />
      <span className="text-[12px]">{cur.label}</span>
      {hover && (
        <Tip>
          {what}: {cur.label} → 누르면 {nxt.label}
        </Tip>
      )}
    </button>
  );
}

/** 켜고 끄는 버튼 */
function Toggle({
  label,
  on,
  onClick,
  children,
}: {
  label: string;
  on: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  const [hover, setHover] = useState(false);
  return (
    <button
      onClick={onClick}
      onPointerEnter={() => setHover(true)}
      onPointerLeave={() => setHover(false)}
      onFocus={() => setHover(true)}
      onBlur={() => setHover(false)}
      aria-label={label}
      aria-pressed={on}
      className={`relative h-control w-control rounded-md inline-flex items-center justify-center
        transition-colors ${on ? "bg-canvas text-accent shadow-sm ring-1 ring-line" : "bg-raised text-fg-mute hover:text-fg"}`}
    >
      {children}
      {hover && (
        <Tip>
          {label} {on ? "끄기" : "켜기"}
        </Tip>
      )}
    </button>
  );
}

export default function ViewBar({
  style,
  onStyle,
  caption,
  onCaption,
  filmstrip,
  onFilmstrip,
}: {
  style: GridStyle;
  onStyle: (s: GridStyle) => void;
  caption: boolean;
  onCaption: (v: boolean) => void;
  filmstrip: boolean;
  onFilmstrip: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center gap-1">
      <Cycle items={STYLES} value={style} onChange={onStyle} what="보기" />
      {hasCaption(style) && (
        <Toggle
          label="이름·크기 표시"
          on={caption}
          onClick={() => onCaption(!caption)}
        >
          <IconCaption className="w-[17px] h-[17px]" />
        </Toggle>
      )}
      <Toggle
        label="필름스트립"
        on={filmstrip}
        onClick={() => onFilmstrip(!filmstrip)}
      >
        <IconFilmstrip className="w-[17px] h-[17px]" />
      </Toggle>
    </div>
  );
}
