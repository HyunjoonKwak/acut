import { useEffect, useRef, useState } from "react";

/** 백엔드 `db::query::Filter`에서 사용자가 고르는 부분만 */
export type Picks = {
  /** 0 사진 · 1 영상 · 2 RAW */
  kind: number | null;
  /** 0 미판정 · 1 남김 · 2 제외 */
  culling_flag: number | null;
  /** 이 값 이상만 */
  min_rating: number | null;
  favorite_only: boolean;
  name_like: string | null;
  /** 사이드바에서 고른 연도 (`2024`) */
  year: string | null;
  /** 사이드바에서 고른 카메라 모델 */
  camera: string | null;
};

export const EMPTY: Picks = {
  kind: null,
  culling_flag: null,
  min_rating: null,
  favorite_only: false,
  name_like: null,
  year: null,
  camera: null,
};

export const isEmpty = (p: Picks) =>
  p.kind === null &&
  p.culling_flag === null &&
  p.min_rating === null &&
  !p.favorite_only &&
  !p.name_like &&
  !p.year &&
  !p.camera;

const KINDS = [
  { v: 0, label: "사진" },
  { v: 1, label: "영상" },
  { v: 2, label: "RAW" },
];
const FLAGS = [
  { v: 1, label: "남김", on: "#F0B429", fg: "#231A00" },
  { v: 2, label: "제외", on: "#E2685C", fg: "#2A0D09" },
  { v: 0, label: "미판정", on: "#3E4A4C", fg: "#EAEFEF" },
];

/**
 * 툴바의 찾기 줄.
 *
 * 스키마와 백엔드 필터는 처음부터 Lap 수준으로 만들어 뒀다 — 여기서 하는 일은
 * 그걸 누를 수 있게 꺼내 놓는 것뿐이다.
 */
export default function FilterBar({
  value,
  onChange,
  children,
}: {
  value: Picks;
  onChange: (p: Picks) => void;
  /** 정렬·그룹 같은 이웃 도구. 같은 줄에 놓는다. */
  children?: React.ReactNode;
}) {
  const [text, setText] = useState(value.name_like ?? "");
  const set = (patch: Partial<Picks>) => onChange({ ...value, ...patch });

  // 한 글자마다 14만 행을 훑지 않게 잠깐 기다린다.
  // value/onChange를 의존성에 넣으면 필터가 바뀔 때마다 타이머가 되감겨
  // 입력이 영영 반영되지 않는다. 최신 값은 ref로 본다.
  const latest = useRef({ value, onChange });
  latest.current = { value, onChange };
  useEffect(() => {
    const t = setTimeout(() => {
      const next = text.trim() || null;
      const { value: v, onChange: fire } = latest.current;
      if (next !== (v.name_like ?? null)) fire({ ...v, name_like: next });
    }, 250);
    return () => clearTimeout(t);
  }, [text]);

  // 밖에서 필터를 비우면 입력칸도 따라 비운다
  useEffect(() => {
    if (value.name_like === null) setText((t) => (t.trim() === "" ? t : ""));
  }, [value.name_like]);

  /// 켜진 칩에 색을 직접 줄 때는 배경 클래스를 비운다 (style이 이긴다)
  const chip = (active: boolean, colored = false) =>
    `h-6 px-2 rounded text-[11.5px] whitespace-nowrap ${
      active
        ? colored
          ? ""
          : "bg-[#2E3739] text-white"
        : "text-[#8D9A9C] hover:text-[#EAEFEF]"
    }`;

  return (
    <div className="h-9 shrink-0 flex items-center gap-1.5 px-3 bg-[#1B2123] border-b border-[#242C2E]">
      <input
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder="파일명"
        className="h-6 w-36 px-2 rounded bg-[#141A1B] text-[12px] text-[#EAEFEF] placeholder:text-[#4E5A5C] outline-none ring-1 ring-[#2A3335] focus:ring-[#49B8B4]"
      />

      <Sep />
      {KINDS.map((k) => (
        <button
          key={k.v}
          onClick={() => set({ kind: value.kind === k.v ? null : k.v })}
          className={chip(value.kind === k.v)}
        >
          {k.label}
        </button>
      ))}

      <Sep />
      {FLAGS.map((f) => {
        const on = value.culling_flag === f.v;
        return (
          <button
            key={f.v}
            onClick={() => set({ culling_flag: on ? null : f.v })}
            className={chip(on, true)}
            style={on ? { background: f.on, color: f.fg } : undefined}
          >
            {f.label}
          </button>
        );
      })}

      <Sep />
      {/* 별점은 "이 값 이상" — 4를 누르면 4·5가 나온다 */}
      <div className="flex items-center gap-0.5">
        {[1, 2, 3, 4, 5].map((n) => (
          <button
            key={n}
            title={`별 ${n}개 이상`}
            onClick={() =>
              set({ min_rating: value.min_rating === n ? null : n })
            }
            className={`w-5 h-6 text-[13px] ${
              (value.min_rating ?? 0) >= n
                ? "text-[#F0B429]"
                : "text-[#3A4547] hover:text-[#5F6C6E]"
            }`}
          >
            ★
          </button>
        ))}
      </div>

      <button
        onClick={() => set({ favorite_only: !value.favorite_only })}
        title="즐겨찾기만"
        className={`w-6 h-6 text-[13px] ${
          value.favorite_only
            ? "text-[#E2685C]"
            : "text-[#3A4547] hover:text-[#5F6C6E]"
        }`}
      >
        ♥
      </button>

      {children && (
        <>
          <div className="flex-1" />
          {children}
        </>
      )}
      {!isEmpty(value) && (
        <>
          <Sep />
          <button
            onClick={() => {
              setText("");
              onChange(EMPTY);
            }}
            className="h-6 px-2 rounded text-[11.5px] text-[#8D9A9C] ring-1 ring-[#333C3F]"
          >
            찾기 해제
          </button>
        </>
      )}
    </div>
  );
}

function Sep() {
  return <span className="w-px h-4 bg-[#2A3335] mx-1" />;
}
