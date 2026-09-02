type Row = { keys: string[]; what: string };

const GRID: Row[] = [
  { keys: ["←", "→", "↑", "↓"], what: "사진 사이 옮겨 다니기" },
  { keys: ["⇧", "+", "방향키"], what: "옮기면서 묶어 고르기" },
  { keys: ["Space"], what: "크게 보기" },
  { keys: ["P"], what: "남김" },
  { keys: ["X"], what: "제외" },
  { keys: ["F"], what: "즐겨찾기" },
  { keys: ["I"], what: "정보 패널 켜기/끄기" },
  { keys: ["0", "–", "5"], what: "별점" },
  { keys: ["C"], what: "나란히 보기 (2장 이상 골랐을 때)" },
  { keys: ["⌘", "A"], what: "불러온 사진 모두 고르기" },
  { keys: ["⌘", "Z"], what: "되돌리기" },
  { keys: ["Esc"], what: "고른 것 풀기" },
];

const VIEWER: Row[] = [
  { keys: ["←", "→"], what: "앞뒤 사진" },
  { keys: ["Space"], what: "확대 / 되돌리기 · 영상은 재생" },
  { keys: ["휠"], what: "커서 자리로 확대 · 끌어서 이동" },
  { keys: ["S"], what: "슬라이드쇼 (아무 키나 누르면 멈춤)" },
  { keys: ["I"], what: "정보 켜고 끄기" },
  { keys: ["\\"], what: "전체화면" },
  { keys: ["P", "X", "F"], what: "남김 · 제외 · 즐겨찾기" },
  { keys: ["0", "–", "5"], what: "별점" },
  { keys: ["Esc"], what: "닫기" },
];

const COMPARE: Row[] = [
  { keys: ["1", "–", "4"], what: "칸 겨누기" },
  { keys: ["←", "→"], what: "옆 칸으로" },
  { keys: ["휠"], what: "함께 확대 · 끌어서 이동" },
  { keys: ["0"], what: "확대 되돌리기" },
  { keys: ["P", "X", "F"], what: "겨눈 칸에 판정" },
];

function Key({ children }: { children: React.ReactNode }) {
  // 「–」와 「+」는 이음말이라 키처럼 그리지 않는다
  if (children === "–" || children === "+")
    return <span className="text-fg-faint px-0.5">{children}</span>;
  return (
    <kbd className="px-1.5 h-5 min-w-[20px] inline-flex items-center justify-center rounded bg-canvas ring-1 ring-line text-[12px] font-mono text-fg-dim">
      {children}
    </kbd>
  );
}

function Table({ title, rows }: { title: string; rows: Row[] }) {
  return (
    <div className="min-w-0">
      <div className="text-[11.5px] uppercase tracking-wider text-fg-mute mb-2">
        {title}
      </div>
      <div className="space-y-1">
        {rows.map((r) => (
          <div key={r.what} className="flex items-center gap-2">
            <span className="flex items-center gap-0.5 shrink-0">
              {r.keys.map((k, i) => (
                <Key key={i}>{k}</Key>
              ))}
            </span>
            <span className="text-[13px] text-fg-dim truncate">{r.what}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

/**
 * 단축키 한 장 — `?`로 연다.
 *
 * 판정은 손이 키보드에 붙어 있어야 빠르다. 그런데 어떤 키가 있는지는
 * 화면 곳곳에 흩어져 있어 한 번 잊으면 다시 찾을 데가 없었다.
 */
export default function Shortcuts({ onClose }: { onClose: () => void }) {
  return (
    <div
      className="fixed inset-0 z-[90] flex items-center justify-center bg-black/50"
      onPointerDown={onClose}
    >
      <div
        onPointerDown={(e) => e.stopPropagation()}
        className="max-w-[720px] w-[90vw] max-h-[80vh] overflow-y-auto rounded-xl
          bg-chrome ring-1 ring-line-strong shadow-2xl p-6"
      >
        <div className="flex items-baseline gap-3 mb-5">
          <span className="text-[16px] font-semibold text-fg">단축키</span>
          <span className="text-[12.5px] text-fg-mute">
            찾기 칸에 글을 쓰는 중에는 듣지 않습니다
          </span>
          <div className="flex-1" />
          <button onClick={onClose} className="text-fg-dim text-[13.5px] px-2">
            닫기 <span className="text-[11px] font-mono">Esc</span>
          </button>
        </div>
        <div className="grid gap-7 sm:grid-cols-3">
          <Table title="목록" rows={GRID} />
          <Table title="크게 보기" rows={VIEWER} />
          <Table title="나란히 보기" rows={COMPARE} />
        </div>
      </div>
    </div>
  );
}
