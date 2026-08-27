import { fmtBytes } from "./format";
import { usePref } from "./prefs";
import { useSelection } from "./selectionStore";
import { Sep } from "./ui";
import { useUi } from "./uiStore";
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
}: {
  rows: FileRow[];
  /** 나란히 놓을 것 — 목록 순서로 앞의 넷 */
  compareIds: number[];
  markPicked: (patch: Mark) => void;
  onTrash: (ids: number[]) => Promise<boolean>;
}) {
  const picked = useSelection((s) => s.picked);
  const clearPicked = useSelection((s) => s.clearPicked);
  const setUi = useUi((s) => s.set);
  const [libId] = usePref("libId");
  if (picked.size === 0) return null;

  const bytes = rows
    .filter((r) => picked.has(r.id))
    .reduce((a, r) => a + r.size, 0);

  return (
    <div className="h-11 shrink-0 flex items-center gap-2 px-3 bg-chrome border-t border-line">
      <span className="text-accent font-semibold tabular-nums text-[13px]">
        {picked.size.toLocaleString()}장 선택
      </span>
      <span className="text-[11.5px] text-fg-mute">{fmtBytes(bytes)}</span>
      <Sep />
      {picked.size >= 2 && (
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
      <div className="flex items-center gap-0.5 px-1">
        {[1, 2, 3, 4, 5].map((n) => (
          <button
            key={n}
            onClick={() => markPicked({ rating: n })}
            title={`별 ${n}개`}
            className="w-5 h-6 text-[13px] text-fg-faint hover:text-keep"
          >
            ★
          </button>
        ))}
      </div>
      <Sep />
      <button
        onClick={() => setUi({ organizing: true })}
        disabled={libId === null}
        title={
          libId === null
            ? "옮겨 넣을 라이브러리를 왼쪽에서 고르세요"
            : undefined
        }
        className="h-control px-3 rounded-md bg-accent text-accent-fg font-semibold text-[12.5px] disabled:opacity-40"
      >
        정리
      </button>
      <button
        onClick={async () => {
          if (await onTrash([...picked])) clearPicked();
        }}
        className="h-control px-3 rounded-md text-drop ring-1 ring-drop text-[12.5px]"
      >
        휴지통으로
      </button>
      <div className="flex-1" />
      <button
        onClick={clearPicked}
        className="h-control px-2 rounded-md text-fg-dim text-[12.5px]"
      >
        선택 해제 <span className="text-[10px] font-mono">Esc</span>
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
      className="h-control px-2.5 rounded-md text-[12.5px] text-fg-dim ring-1 ring-line-strong hover:text-white"
    >
      {children}
      {hint && (
        <span className="ml-1 text-[10px] font-mono text-fg-mute">{hint}</span>
      )}
    </button>
  );
}
