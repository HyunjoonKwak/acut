import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useData } from "./dataStore";
import type { Facet } from "./FacetList";
import type { Picks } from "./picks";

/**
 * 위치 갈래 — 국가 › 시도 › 시군구 세 단계.
 *
 * 전에는 `37.2,127.1` 같은 좌표 격자를 그대로 늘어놓아 어디가 어딘지 알 수
 * 없었다 (2026-09-01 지적). 이제 «지명 채우기»가 넣어 둔 이름으로 묶는다.
 *
 * 각 단계는 **위 단계를 필터에 건 채로** 센다 — 경기도를 펼치면 경기도 안의
 * 시군구만, 그 장수도 지금 걸린 다른 조건(연도·태그…) 안에서 센 값이다.
 */
export default function PlaceTree({
  picks,
  facetFilter,
  onPick,
  unnamed,
}: {
  picks: Picks;
  /** 지명 조건을 뺀 필터 — 이 안에서 센다 */
  facetFilter: unknown;
  onPick: (p: { country: string | null; admin1: string | null; admin2: string | null }) => void;
  /** 아직 이름이 없는 사진 수 — 0보다 크면 안내를 보인다 */
  unnamed: number;
}) {
  const country = picks.country;
  const admin1 = picks.admin1;

  const [countries, setCountries] = useState<Facet[] | null>(null);
  const [regions, setRegions] = useState<{ of: string; items: Facet[] } | null>(null);
  const [cities, setCities] = useState<{ of: string; items: Facet[] } | null>(null);

  // 지명이 채워지면 개정 번호가 올라 세 단계가 모두 다시 센다
  const geoRev = useData((s) => s.geoRev);
  const key = useMemo(() => JSON.stringify([facetFilter, geoRev]), [facetFilter, geoRev]);

  useEffect(() => {
    let live = true;
    invoke<Facet[]>("files_facets", { filter: facetFilter, kind: "country" })
      .then((f) => live && setCountries(f))
      .catch(() => live && setCountries([]));
    return () => {
      live = false;
    };
  }, [facetFilter, key]);

  // 펼친 국가의 시도
  useEffect(() => {
    if (country === null) return;
    let live = true;
    invoke<Facet[]>("files_facets", {
      filter: { ...(facetFilter as object), country },
      kind: "admin1",
    })
      .then((f) => live && setRegions({ of: country, items: f }))
      .catch(() => live && setRegions({ of: country, items: [] }));
    return () => {
      live = false;
    };
  }, [country, facetFilter, key]);

  // 펼친 시도의 시군구
  useEffect(() => {
    if (country === null || admin1 === null) return;
    let live = true;
    invoke<Facet[]>("files_facets", {
      filter: { ...(facetFilter as object), country, admin1 },
      kind: "admin2",
    })
      .then((f) => live && setCities({ of: `${country}/${admin1}`, items: f }))
      .catch(() => live && setCities({ of: `${country}/${admin1}`, items: [] }));
    return () => {
      live = false;
    };
  }, [country, admin1, facetFilter, key]);

  if (countries === null) {
    return <div className="px-3 py-2 text-[13px] text-fg-mute">세는 중…</div>;
  }

  const total = countries.reduce((a, c) => a + c.count, 0);

  return (
    <div className="py-1">
      {unnamed > 0 && (
        <div className="px-3 pb-2 text-[12px] text-fg-mute leading-snug">
          아직 이름이 없는 사진 {unnamed.toLocaleString()}장 — 설정 › 탐색의
          «지명 채우기»로 좌표에 지명을 붙입니다.
        </div>
      )}
      <Row
        label="모든 위치"
        count={total}
        on={country === null && admin1 === null && picks.admin2 === null}
        onClick={() => onPick({ country: null, admin1: null, admin2: null })}
      />
      {countries.map((c) => {
        const open = country === c.value;
        return (
          <div key={c.value || "(없음)"}>
            <Row
              label={c.label}
              count={c.count}
              depth={0}
              caret={c.value ? (open ? "▼" : "▶") : undefined}
              on={open && admin1 === null}
              onClick={() =>
                onPick({ country: open ? null : c.value, admin1: null, admin2: null })
              }
            />
            {open &&
              regions?.of === c.value &&
              regions.items.map((r) => {
                const openR = admin1 === r.value;
                return (
                  <div key={r.value || "(없음)"}>
                    <Row
                      label={r.label}
                      count={r.count}
                      depth={1}
                      caret={r.value ? (openR ? "▼" : "▶") : undefined}
                      on={openR && picks.admin2 === null}
                      onClick={() =>
                        onPick({
                          country: c.value,
                          admin1: openR ? null : r.value,
                          admin2: null,
                        })
                      }
                    />
                    {openR &&
                      cities?.of === `${c.value}/${r.value}` &&
                      cities.items.map((t) => (
                        <Row
                          key={t.value || "(없음)"}
                          label={t.label}
                          count={t.count}
                          depth={2}
                          on={picks.admin2 === t.value}
                          onClick={() =>
                            onPick({
                              country: c.value,
                              admin1: r.value,
                              admin2: picks.admin2 === t.value ? null : t.value,
                            })
                          }
                        />
                      ))}
                  </div>
                );
              })}
          </div>
        );
      })}
      {countries.length === 0 && (
        <div className="px-3 py-2 text-[13px] text-fg-mute">없음</div>
      )}
    </div>
  );
}

function Row({
  label,
  count,
  on,
  onClick,
  depth = 0,
  caret,
}: {
  label: string;
  count: number;
  on: boolean;
  onClick: () => void;
  depth?: number;
  caret?: string;
}) {
  return (
    <button
      onClick={onClick}
      title={label}
      className={`w-full flex items-center gap-1.5 py-1.5 pr-3 text-[13.5px] ${
        on ? "bg-raised text-fg" : "text-fg-dim hover:text-fg hover:bg-hover"
      }`}
      style={{ paddingLeft: 12 + depth * 12 }}
    >
      <span className="w-3 shrink-0 text-[10px] text-fg-mute">{caret ?? ""}</span>
      <span className="flex-1 text-left truncate">{label}</span>
      <span className="text-fg-mute tabular-nums text-[12px] shrink-0">
        {count.toLocaleString()}
      </span>
    </button>
  );
}
