import { useState } from "react";
import {
  IconCaption,
  IconCard,
  IconContain,
  IconFill,
  IconFilmstrip,
  IconJustified,
  IconStretch,
  IconTile,
} from "./icons";
import type { GridStyle, Scaling } from "./gridStyle";

/**
 * 보기 방식 — 툴바의 아이콘 묶음.
 *
 * 드롭다운 하나에 넣어 뒀더니 지금 어느 보기인지 열어 봐야 알 수 있었다.
 * 자주 오가는 설정은 열지 않고 보이고, 한 번에 눌려야 한다. 묶음 안에서
 * 켜진 것이 하나 도드라진다 — 라디오 버튼과 같은 뜻이다.
 *
 * 이름표는 커서가 왔을 때만 뜬다. 레일과 같은 방식이다.
 */

const STYLE_ITEMS: {
  v: GridStyle;
  label: string;
  Icon: (p: { className?: string }) => React.ReactElement;
}[] = [
  { v: "card", label: "카드 보기", Icon: IconCard },
  { v: "tile", label: "타일 보기", Icon: IconTile },
  { v: "justified", label: "양쪽 맞춤", Icon: IconJustified },
];

const SCALING_ITEMS: {
  v: Scaling;
  label: string;
  Icon: (p: { className?: string }) => React.ReactElement;
}[] = [
  { v: "cover", label: "채우기 — 넘치는 부분은 자릅니다", Icon: IconFill },
  { v: "contain", label: "사진 전체 — 비를 지킵니다", Icon: IconContain },
  { v: "fill", label: "늘리기 — 비를 무시합니다", Icon: IconStretch },
];

function Tip({ children }: { children: React.ReactNode }) {
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

function Icon({
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
      className={`relative h-control w-control rounded inline-flex items-center justify-center
        transition-colors ${
          on ? "bg-canvas text-accent shadow-sm" : "text-fg-mute hover:text-fg"
        }`}
    >
      {children}
      {hover && <Tip>{label}</Tip>}
    </button>
  );
}

/** 라디오처럼 묶인 한 벌 */
function Group({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-0.5 p-0.5 rounded-md bg-raised">
      {children}
    </div>
  );
}

export default function ViewBar({
  style,
  scaling,
  onStyle,
  onScaling,
  caption,
  onCaption,
  filmstrip,
  onFilmstrip,
}: {
  style: GridStyle;
  scaling: Scaling;
  onStyle: (s: GridStyle) => void;
  onScaling: (s: Scaling) => void;
  caption: boolean;
  onCaption: (v: boolean) => void;
  filmstrip: boolean;
  onFilmstrip: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center gap-1.5">
      <Group>
        {STYLE_ITEMS.map(({ v, label, Icon: I }) => (
          <Icon
            key={v}
            label={label}
            on={style === v}
            onClick={() => onStyle(v)}
          >
            <I className="w-[17px] h-[17px]" />
          </Icon>
        ))}
      </Group>

      {/* 양쪽 맞춤은 사진 비를 지키므로 담는 방식이 의미 없다 */}
      {style !== "justified" && (
        <Group>
          {SCALING_ITEMS.map(({ v, label, Icon: I }) => (
            <Icon
              key={v}
              label={label}
              on={scaling === v}
              onClick={() => onScaling(v)}
            >
              <I className="w-[17px] h-[17px]" />
            </Icon>
          ))}
        </Group>
      )}

      <Group>
        <Icon
          label="이름·크기 표시"
          on={caption}
          onClick={() => onCaption(!caption)}
        >
          <IconCaption className="w-[17px] h-[17px]" />
        </Icon>
        <Icon
          label="필름스트립"
          on={filmstrip}
          onClick={() => onFilmstrip(!filmstrip)}
        >
          <IconFilmstrip className="w-[17px] h-[17px]" />
        </Icon>
      </Group>
    </div>
  );
}
