import { fmtBytes } from "./format";
import { useViewportW } from "./useViewportW";
import { usePref } from "./prefs";
import { useSelection } from "./selectionStore";
import { Sep } from "./ui";
import { useUi } from "./uiStore";
import { useData } from "./dataStore";
import { useView } from "./viewStore";
import { areaLabel, nextArea } from "./areaItems";
import type { FileRow, Mark } from "./types";

/**
 * 선택 패널 — 무엇이 골라져 있고 무엇을 할 수 있는지 (Lap의 SelectionPanel).
 * 골라 둔 것이 있을 때만 상태바 위에 뜬다.
 */
export default function SelectionPanel({
  rows,
  compareIds,
  markPicked,
  onTrash,
  onRestore,
  onDelete,
}: {
  rows: FileRow[];
  /** 나란히 놓을 것 — 목록 순서로 앞의 넷 */
  compareIds: number[];
  markPicked: (patch: Mark) => void;
  onTrash: (ids: number[]) => Promise<boolean>;
  /** 휴지통 화면에서 — 고른 것만 되돌리기 / 영구히 지우기 */
  onRestore?: (ids: number[]) => Promise<boolean>;
  onDelete?: (ids: number[]) => Promise<boolean>;
}) {
  // 좁은 창 — 별점·나란히 보기를 접어 핵심(남김·제외·정리·휴지통)이 밀려나지 않게.
  // 훅이라 이른 반환(휴지통 모드)보다 먼저 부른다
  const narrow = useViewportW() < 880;
  const picked = useSelection((s) => s.picked);
  const clearPicked = useSelection((s) => s.clearPicked);
  const viewTrash = useView((s) => s.viewTrash);
  const setUi = useUi((s) => s.set);
  const [libId] = usePref("libId");
  const libs = useData((s) => s.libs);
  // 지금 라이브러리의 흐름상 다음 칸 — 작업대면 «내사진으로», 내사진이면 «공용으로»
  const next = nextArea(libs.find((l) => l.id === libId)?.area ?? 3);
  if (picked.size === 0) return null;

  const bytes = rows
    .filter((r) => picked.has(r.id))
    .reduce((a, r) => a + r.size, 0);

  // 휴지통에서는 «되돌릴지 / 영구히 지울지»만 — 남김·제외 판정은 여기서 할 일이 아니다 (사용자 지적 2026-08-30)
  if (viewTrash) {
    return (
      <div className="h-11 shrink-0 flex items-center gap-2 px-3 bg-chrome border-t border-line bar-fixed">
        <div className="w-[176px] shrink-0 flex items-baseline gap-2 tabular-nums overflow-hidden">
          <span className="text-accent font-semibold text-[14px] whitespace-nowrap">
            {picked.size.toLocaleString()}장 선택
          </span>
          <span className="text-[12.5px] text-fg-mute whitespace-nowrap truncate">{fmtBytes(bytes)}</span>
        </div>
        <Sep />
        <button
          onClick={async () => {
            if (await onRestore?.([...picked])) clearPicked();
          }}
          title="고른 사진을 원래 폴더로 되돌립니다"
          className="h-control px-3 rounded-md bg-accent text-accent-fg font-semibold text-[13.5px]"
        >
          되돌리기
        </button>
        <button
          onClick={async () => {
            if (await onDelete?.([...picked])) clearPicked();
          }}
          title="고른 사진을 디스크에서 영구히 지웁니다 — 되돌릴 수 없습니다"
          className="h-control px-3 rounded-md text-drop ring-1 ring-drop text-[13.5px]"
        >
          영구히 지우기
        </button>
        <div className="flex-1" />
        <button onClick={clearPicked} className="h-control px-2 rounded-md text-fg-dim text-[13.5px]">
          선택 해제 <span className="text-[11px] font-mono">Esc</span>
        </button>
      </div>
    );
  }

  return (
    <div className="h-11 shrink-0 flex items-center gap-2 px-3 bg-chrome border-t border-line bar-fixed">
      {/* 장수·용량은 폭을 못박는다 — 사진마다 글자 길이가 달라 뒤의 버튼들이
          흔들리면 키보드로 빠르게 넘길 때 어지럽다 */}
      <div className="w-[176px] shrink-0 flex items-baseline gap-2 tabular-nums overflow-hidden">
        <span className="text-accent font-semibold text-[14px] whitespace-nowrap">
          {picked.size.toLocaleString()}장 선택
        </span>
        <span className="text-[12.5px] text-fg-mute whitespace-nowrap truncate">
          {fmtBytes(bytes)}
        </span>
      </div>
      <Sep />
      {picked.size >= 2 && !narrow && (
        <PanelBtn onClick={() => setUi({ comparing: compareIds })}>
          나란히 보기
        </PanelBtn>
      )}
      <PanelBtn onClick={() => markPicked({ cullingFlag: 1 })} hint="P">
        남김
      </PanelBtn>
      <PanelBtn onClick={() => markPicked({ cullingFlag: 2 })} hint="X">
        제외
      </PanelBtn>
      <PanelBtn onClick={() => markPicked({ favorite: true })} hint="F">
        즐겨찾기
      </PanelBtn>
      {!narrow && (
      <div className="flex items-center gap-0.5 px-1">
        {[1, 2, 3, 4, 5].map((n) => (
          <button
            key={n}
            onClick={() => markPicked({ rating: n })}
            title={`별 ${n}개`}
            className="w-5 h-6 text-[14px] text-fg-faint hover:text-keep"
          >
            ★
          </button>
        ))}
      </div>
      )}
      <Sep />
      <button
        onClick={() => setUi({ organizing: true })}
        disabled={libId === null}
        title={
          libId === null
            ? "옮겨 넣을 라이브러리를 왼쪽에서 고르세요"
            : undefined
        }
        className="h-control px-3 rounded-md bg-accent text-accent-fg font-semibold text-[13.5px] disabled:opacity-40"
      >
        {next === null ? "정리" : `${areaLabel(next)}으로 정리`}
      </button>
      <button
        onClick={async () => {
          if (await onTrash([...picked])) clearPicked();
        }}
        className="h-control px-3 rounded-md text-drop ring-1 ring-drop text-[13.5px]"
      >
        휴지통으로
      </button>
      <div className="flex-1" />
      <button
        onClick={clearPicked}
        className="h-control px-2 rounded-md text-fg-dim text-[13.5px]"
      >
        선택 해제 <span className="text-[11px] font-mono">Esc</span>
      </button>
    </div>
  );
}

/// 선택 패널의 작은 버튼
function PanelBtn({
  children,
  hint,
  onClick,
}: {
  children: React.ReactNode;
  hint?: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="h-control px-2.5 rounded-md text-[13.5px] text-fg-dim ring-1 ring-line-strong hover:text-white"
    >
      {children}
      {hint && (
        <span className="ml-1 text-[11px] font-mono text-fg-mute">{hint}</span>
      )}
    </button>
  );
}
