import { useData } from "./dataStore";
import { fmtBytes } from "./format";
import { usePref } from "./prefs";
import { useView } from "./viewStore";

/**
 * 휴지통 화면의 왼쪽 패널 — 라이브러리마다의 휴지통을 한 줄씩.
 *
 * 휴지통은 라이브러리마다 따로 있다(같은 디스크 안 `.acut/휴지통` — 디스크를 건너
 * 복사하지 않으려고). 고른 라이브러리 것만 보이면 다른 쪽에 남은 것을 빠뜨리므로
 * (2026-08-30: T7 을 비우고 공용 1,904장이 남아 있었다) 전부를 줄 세우고 눌러 옮겨 간다.
 */
export default function TrashPanel() {
  const [libId, setLibId] = usePref("libId");
  const rows = useData((s) => s.trashByLib);
  const setSel = useView((s) => s.setSel);
  const total = rows.reduce((a, r) => a + r.files, 0);
  const bytes = rows.reduce((a, r) => a + r.bytes, 0);
  // 휴지통 화면에선 폴더 선택이 남아 있으면 그 폴더 것만 보인다 — 라이브러리로 옮길 땐 푼다
  const go = (id: number | null) => {
    setSel(null);
    setLibId(id);
  };
  return (
    <div className="py-1">
      <Row
        label="모든 라이브러리"
        files={total}
        bytes={bytes}
        on={libId === null}
        onClick={() => go(null)}
      />
      {rows.map((r) => (
        <Row
          key={r.library_id}
          label={r.name}
          files={r.files}
          bytes={r.bytes}
          on={libId === r.library_id}
          onClick={() => go(r.library_id)}
        />
      ))}
      <p className="px-3 pt-3 text-[12px] text-fg-mute leading-snug">
        휴지통은 라이브러리마다 따로 있습니다 — 같은 디스크 안{" "}
        <code>.acut/휴지통</code>. 되돌리기·영구히 비우기는 지금 보는 라이브러리
        것에만 듭니다.
      </p>
    </div>
  );
}

function Row({
  label,
  files,
  bytes,
  on,
  onClick,
}: {
  label: string;
  files: number;
  bytes: number;
  on: boolean;
  onClick: () => void;
}) {
  const empty = files === 0;
  return (
    <button
      onClick={onClick}
      className={`w-full flex items-center gap-2 px-3 py-1.5 text-[13.5px] ${
        on ? "bg-raised text-fg" : "text-fg-dim hover:text-fg hover:bg-chrome"
      }`}
    >
      <span className="flex-1 text-left truncate">{label}</span>
      <span
        className={`tabular-nums text-[12px] whitespace-nowrap ${
          empty ? "text-fg-mute" : on ? "text-fg" : "text-fg-dim"
        }`}
      >
        {empty
          ? "비어 있음"
          : `${files.toLocaleString()}장 · ${fmtBytes(bytes)}`}
      </span>
    </button>
  );
}
