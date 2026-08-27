import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type Facet = { value: string; label: string; count: number };
export type FacetKind = "year" | "camera" | "rating" | "kind";

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
  const [items, setItems] = useState<Facet[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let live = true;
    setLoading(true);
    invoke<Facet[]>("files_facets", { filter, kind })
      .then((f) => live && setItems(f))
      .catch(() => live && setItems([]))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
  }, [kind, filter]);

  if (loading && items.length === 0) {
    return <div className="px-3 py-2 text-[12px] text-[#5F6C6E]">세는 중…</div>;
  }
  if (items.length === 0) {
    return <div className="px-3 py-2 text-[12px] text-[#5F6C6E]">없음</div>;
  }

  return (
    <>
      <button
        onClick={() => onPick(null)}
        className={`w-full text-left px-3 py-1.5 text-[12.5px] ${
          selected === null ? "bg-[#232A2C] text-white" : "text-[#A3B2B4]"
        }`}
      >
        전체{" "}
        <span className="text-[#6D7B7E] tabular-nums float-right">
          {items.reduce((a, f) => a + f.count, 0).toLocaleString()}
        </span>
      </button>
      {items.map((f) => (
        <button
          key={f.value}
          onClick={() => onPick(selected === f.value ? null : f.value)}
          title={f.label}
          className={`w-full text-left px-3 py-1 text-[12.5px] truncate ${
            selected === f.value ? "bg-[#232A2C] text-white" : "text-[#A3B2B4]"
          }`}
        >
          {f.label}{" "}
          <span className="text-[#5F6C6E] tabular-nums text-[11px] float-right">
            {f.count.toLocaleString()}
          </span>
        </button>
      ))}
    </>
  );
}
