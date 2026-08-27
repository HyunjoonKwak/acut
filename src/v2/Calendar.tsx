import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Bucket } from "./ScrollBar";
import type { Facet } from "./FacetList";

/**
 * 날짜 갈래 — 연도를 펴면 달, 달을 펴면 날이 나온다.
 *
 * 평평한 연도 목록만 있으면 "2019년 8월"로 바로 못 간다. Lap의 Calendar와
 * 같은 얼개다. 눈금(월별 장수)은 타임라인 스크롤바가 이미 읽어 둔 것을
 * 그대로 쓴다 — 같은 값을 두 번 세지 않는다.
 */
export default function Calendar({
  buckets,
  year,
  month,
  day,
  facetFilter,
  onPick,
}: {
  buckets: Bucket[];
  /** 고른 연도 (`2024`). null이면 전체 */
  year: string | null;
  /** 고른 월 (`2024-08`). null이면 그 해 전체 */
  month: string | null;
  /** 고른 날 (`2024-08-27`). null이면 그 달 전체 */
  day: string | null;
  /** 날짜별 장수를 셀 때 쓰는 필터 (날짜 조건은 뺀 것) */
  facetFilter: unknown;
  onPick: (
    year: string | null,
    month: string | null,
    day: string | null,
  ) => void;
}) {
  const [open, setOpen] = useState<Set<string>>(
    () => new Set(year ? [year] : []),
  );
  /// 펼친 달 (`2024-08`) 하나. 두 달을 동시에 펼치면 목록이 길어 길을 잃는다.
  const [openMonth, setOpenMonth] = useState<string | null>(month);
  const [days, setDays] = useState<{ month: string; items: Facet[] } | null>(
    null,
  );

  // 펼친 달의 날짜별 장수 — 그 달로 좁힌 필터로 센다
  useEffect(() => {
    if (!openMonth) return;
    let live = true;
    const f = { ...(facetFilter as object), month: openMonth };
    invoke<Facet[]>("files_facets", { filter: f, kind: "day" })
      .then((items) => live && setDays({ month: openMonth, items }))
      .catch(() => live && setDays({ month: openMonth, items: [] }));
    return () => {
      live = false;
    };
  }, [openMonth, facetFilter]);

  const years = useMemo(() => {
    const m = new Map<string, { count: number; months: Bucket[] }>();
    for (const b of buckets) {
      const y = String(b.year);
      const e = m.get(y) ?? { count: 0, months: [] };
      e.count += b.count;
      e.months.push(b);
      m.set(y, e);
    }
    return [...m.entries()].sort((a, b) => b[0].localeCompare(a[0]));
  }, [buckets]);

  if (years.length === 0) {
    return <div className="px-3 py-2 text-[12px] text-fg-mute">없음</div>;
  }

  const total = years.reduce((a, [, v]) => a + v.count, 0);

  return (
    <>
      <Row
        label="전체"
        count={total}
        on={year === null}
        onClick={() => onPick(null, null, null)}
      />
      {years.map(([y, v]) => {
        const expanded = open.has(y);
        return (
          <div key={y}>
            <div
              className={`flex items-center pr-2 ${
                year === y && month === null ? "bg-raised" : ""
              }`}
            >
              <button
                onClick={() =>
                  setOpen((p) => {
                    const n = new Set(p);
                    if (n.has(y)) n.delete(y);
                    else n.add(y);
                    return n;
                  })
                }
                className="w-5 shrink-0 text-[9px] text-fg-mute hover:text-fg"
              >
                {expanded ? "▼" : "▶"}
              </button>
              <button
                onClick={() => onPick(y, null, null)}
                className={`flex-1 min-w-0 text-left py-1 text-[12.5px] ${
                  year === y ? "text-fg" : "text-fg-dim"
                }`}
              >
                {y}년
              </button>
              <span className="text-fg-faint tabular-nums text-[11px]">
                {v.count.toLocaleString()}
              </span>
            </div>
            {expanded &&
              v.months
                .slice()
                .sort((a, b) => b.month - a.month)
                .map((b) => {
                  const key = `${y}-${String(b.month).padStart(2, "0")}`;
                  const expanded = openMonth === key;
                  return (
                    <div key={key}>
                      <div
                        className={`flex items-center pr-2 ${
                          month === key && day === null ? "bg-raised" : ""
                        }`}
                      >
                        <button
                          onClick={() => setOpenMonth(expanded ? null : key)}
                          className="w-8 shrink-0 pl-3 text-left text-[9px] text-fg-mute hover:text-fg"
                        >
                          {expanded ? "▼" : "▶"}
                        </button>
                        <button
                          onClick={() =>
                            onPick(y, month === key ? null : key, null)
                          }
                          className={`flex-1 min-w-0 text-left py-1 text-[12.5px] ${
                            month === key ? "text-fg" : "text-fg-dim"
                          }`}
                        >
                          {b.month}월
                        </button>
                        <span className="text-fg-faint tabular-nums text-[11px]">
                          {b.count.toLocaleString()}
                        </span>
                      </div>
                      {expanded &&
                        days?.month === key &&
                        days.items.map((d) => (
                          <Row
                            key={d.value}
                            indent={2}
                            label={d.label}
                            count={d.count}
                            on={day === d.value}
                            onClick={() =>
                              onPick(y, key, day === d.value ? null : d.value)
                            }
                          />
                        ))}
                    </div>
                  );
                })}
          </div>
        );
      })}
    </>
  );
}

function Row({
  label,
  count,
  on,
  indent,
  onClick,
}: {
  label: string;
  count: number;
  on: boolean;
  /** 들여쓰기 단계 — 1은 달, 2는 날 */
  indent?: number | boolean;
  onClick: () => void;
}) {
  const level = indent === true ? 1 : indent || 0;
  return (
    <button
      onClick={onClick}
      className={`w-full flex items-center pr-2 py-1 text-[12.5px] ${
        level === 2 ? "pl-12" : level === 1 ? "pl-8" : "pl-3"
      } ${on ? "bg-raised text-fg" : "text-fg-dim hover:text-fg"}`}
    >
      <span className="flex-1 text-left truncate">{label}</span>
      <span className="text-fg-faint tabular-nums text-[11px]">
        {count.toLocaleString()}
      </span>
    </button>
  );
}
