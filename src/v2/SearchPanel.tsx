import { useDebouncedText } from "./useDebouncedText";
import type { Picks } from "./picks";
import FacetList from "./FacetList";
import { useState } from "react";
import { useUi } from "./uiStore";

const KINDS = [
  { v: 0, label: "사진" },
  { v: 1, label: "영상" },
  { v: 2, label: "RAW" },
];
const FLAGS = [
  { v: 1, label: "남김" },
  { v: 2, label: "제외" },
  { v: 0, label: "미판정" },
];

function Chip({
  on,
  onClick,
  children,
}: {
  on: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={`h-control px-2 rounded-md text-[13px] ${
        on ? "bg-accent text-accent-fg" : "bg-raised text-fg-dim hover:text-fg"
      }`}
    >
      {children}
    </button>
  );
}

function Head({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-3 pt-3 pb-1 text-[11.5px] uppercase tracking-wider text-fg-mute">
      {children}
    </div>
  );
}

/**
 * 찾기 갈래 — 툴바 팝오버와 같은 조건을 사이드바에 펼쳐 둔 것.
 *
 * 팝오버는 한 번 고르고 닫는 자리다. 조건을 여러 개 겹쳐 가며 다듬을 때는
 * 계속 열려 있는 편이 낫다. 값은 같은 `Picks`를 공유하므로 어디서 바꾸든
 * 양쪽에 함께 반영된다.
 */
export default function SearchPanel({
  value,
  onChange,
  facetFilter,
}: {
  value: Picks;
  onChange: (p: Picks) => void;
  facetFilter: unknown;
}) {
  const set = (patch: Partial<Picks>) => onChange({ ...value, ...patch });

  // 한 글자마다 전체를 훑지 않게 타이핑이 멎기를 기다린다
  const [text, setText] = useDebouncedText(value.name_like ?? "", 250, (t) => {
    const next = t.trim() || null;
    if (next !== value.name_like) onChange({ ...value, name_like: next });
  });

  // 글로 찾기 — Enter로 묻는다. 한 글자마다 모델을 돌리기엔 무겁다.
  const [ai, setAi] = useState("");
  const ask = () => {
    const q = ai.trim();
    if (q) useUi.getState().set({ textSearch: q });
  };

  return (
    <>
      <div className="px-2">
        <input
          value={ai}
          onChange={(e) => setAi(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") ask();
          }}
          placeholder="AI로 찾기 — «바닷가 강아지» ⏎"
          title="글로 찾습니다. 한국어·영어 다 됩니다. 설정 › AI에서 글로 찾기 모델을 받아야 합니다."
          className="w-full h-control px-2 rounded-md bg-canvas text-[13px] text-fg
            placeholder:text-fg-faint outline-none ring-1 ring-line focus:ring-accent"
        />
      </div>
      <div className="px-2 mt-1.5">
        <input
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="파일 이름"
          className="w-full h-control px-2 rounded-md bg-canvas text-[13px] text-fg
            placeholder:text-fg-faint outline-none ring-1 ring-line focus:ring-accent"
        />
      </div>

      <Head>종류</Head>
      <div className="px-2 flex flex-wrap gap-1">
        {KINDS.map((k) => (
          <Chip
            key={k.v}
            on={value.kind === k.v}
            onClick={() => set({ kind: value.kind === k.v ? null : k.v })}
          >
            {k.label}
          </Chip>
        ))}
      </div>

      <Head>판정</Head>
      <div className="px-2 flex flex-wrap gap-1">
        {FLAGS.map((f) => (
          <Chip
            key={f.v}
            on={value.culling_flag === f.v}
            onClick={() =>
              set({ culling_flag: value.culling_flag === f.v ? null : f.v })
            }
          >
            {f.label}
          </Chip>
        ))}
        <Chip
          on={value.favorite_only}
          onClick={() => set({ favorite_only: !value.favorite_only })}
        >
          ♥ 즐겨찾기
        </Chip>
      </div>

      <Head>평점</Head>
      <FacetList
        kind="rating"
        filter={facetFilter}
        selected={value.min_rating === null ? null : String(value.min_rating)}
        onPick={(v) => set({ min_rating: v === null ? null : Number(v) })}
      />
    </>
  );
}
