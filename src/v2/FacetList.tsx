import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type Facet = { value: string; label: string; count: number };
export type FacetKind =
  "year" | "camera" | "lens" | "rating" | "kind" | "place";

/**
 * 사이드바의 갈래 목록 — 연도·카메라·평점.
 *
 * 장수를 **지금 필터 안에서** 센다. 그래야 눌러도 0장인 항목이 안 나온다.
 */
export default function FacetList({
  kind,
  filter,
  selected,
  onPick,
}: {
  kind: FacetKind;
  /** 지금 걸린 필터. 이 안에서 센다. */
  filter: unknown;
  selected: string | null;
  onPick: (value: string | null) => void;
}) {
  // 결과를 «어느 조건에 대한 것인지»와 함께 둔다. 조건이 바뀌면 열쇠가
  // 안 맞아 저절로 «세는 중»이 된다 — 효과 안에서 loading을 따로 켤 일이 없다.
  const key = useMemo(() => JSON.stringify([kind, filter]), [kind, filter]);
  const [got, setGot] = useState<{ key: string; items: Facet[] } | null>(null);

  useEffect(() => {
    let live = true;
    invoke<Facet[]>("files_facets", { filter, kind })
      .then((f) => live && setGot({ key, items: f }))
      .catch(() => live && setGot({ key, items: [] }));
    return () => {
      live = false;
    };
  }, [kind, filter, key]);

  const items = got?.key === key ? got.items : (got?.items ?? []);
  const loading = got?.key !== key;

  if (loading && items.length === 0) {
    return <div className="px-3 py-2 text-[12px] text-fg-mute">세는 중…</div>;
  }
  if (items.length === 0) {
    return <div className="px-3 py-2 text-[12px] text-fg-mute">없음</div>;
  }

  return (
    <>
      <button
        onClick={() => onPick(null)}
        className={`w-full text-left px-3 py-1.5 text-[12.5px] ${
          selected === null ? "bg-raised text-fg" : "text-fg-dim"
        }`}
      >
        전체{" "}
        <span className="text-fg-mute tabular-nums float-right">
          {items.reduce((a, f) => a + f.count, 0).toLocaleString()}
        </span>
      </button>
      {items.map((f) => (
        <button
          key={f.value}
          onClick={() => onPick(selected === f.value ? null : f.value)}
          title={f.label}
          className={`w-full text-left px-3 py-1 text-[12.5px] truncate ${
            selected === f.value ? "bg-raised text-fg" : "text-fg-dim"
          }`}
        >
          {f.label}{" "}
          <span className="text-fg-mute tabular-nums text-[11px] float-right">
            {f.count.toLocaleString()}
          </span>
        </button>
      ))}
    </>
  );
}
