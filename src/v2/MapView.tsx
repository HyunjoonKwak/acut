import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { thumbUrlOf } from "./types";
import L from "leaflet";
import "leaflet/dist/leaflet.css";
import type { Filter } from "./viewStore";
import { useView } from "./viewStore";
import {
  bboxString,
  cellBounds,
  isFinest,
  parseBbox,
  precisionForZoom,
  safeMapBbox,
} from "./mapMath";

type Cell = {
  lat: number;
  lon: number;
  n: number;
  library_id: number | null;
  thumb: string | null;
  /** 이 자리에서 가장 흔한 지명 — 아직 지명을 안 채웠으면 null */
  place: string | null;
  /** 이 자리에 섞인 서로 다른 지명 수 */
  places: number;
};

/** 마커에 올린 손가락 아래에 뜨는 글 — 위경도 대신 어디인지를 말한다 */
function pinLabel(c: Cell, fine: boolean): string {
  const where =
    c.place === null
      ? ""
      : c.places > 1
        ? `${c.place} 외 ${(c.places - 1).toLocaleString()}곳 · `
        : `${c.place} · `;
  return `${where}${c.n.toLocaleString()}장 — ${fine ? "누르면 이 자리의 사진만" : "누르면 확대"}`;
}

type Overview = {
  total: number;
  bounds: [number, number, number, number] | null;
};

const thumbUrl = (c: Cell) =>
  c.thumb && c.library_id !== null ? thumbUrlOf(c.library_id, c.thumb) : null;

/** 타일은 온라인 — 사용자 결정(2026-08-27). 어두운 바탕이 앱과 맞는다. */
// Carto 무료 베이스맵은 1x·@2x 모두 «API KEY REQUIRED» 워터마크를 박는다
// (2026-08-31 타일 실측). 키 없는 OSM 표준 타일로 바꾸고, 어두운 톤은 CSS 필터
// (.acut-map-tile, index.css)가 만든다.
const TILES = {
  url: "https://tile.openstreetmap.org/{z}/{x}/{y}.png",
  attribution:
    '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
} as const;

/**
 * 지도 — 사진이 찍힌 자리를 칸으로 보인다.
 *
 * 칸은 확대할수록 잘게 다시 묻는다 (mapMath.precisionForZoom). 굵은 칸을
 * 누르면 그리로 확대, 가장 잘은 칸을 누르면 그 영역의 사진만 아래 그리드에
 * 남는다. 「보이는 영역만」은 지금 화면을 그대로 조건으로.
 *
 * Leaflet 지도는 React 밖에 산다 — ref로 한 번 만들고, 칸이 바뀌면 마커만
 * 갈아 끼운다.
 */
export default function MapView({ filter }: { filter: Filter }) {
  const box = useRef<HTMLDivElement>(null);
  const map = useRef<L.Map | null>(null);
  const layer = useRef<L.LayerGroup | null>(null);
  const picked = useRef<L.Rectangle | null>(null);
  const fittedFilter = useRef<string | null>(null);
  const [view, setView] = useState<{ zoom: number; tick: number }>({
    zoom: 6,
    tick: 0,
  });
  const [offline, setOffline] = useState(false);
  const [retry, setRetry] = useState(0);
  const [overviewState, setOverviewState] = useState<{
    key: string;
    total: number | null;
    failed: boolean;
  }>({ key: "", total: null, failed: false });
  const [cellsState, setCellsState] = useState({ key: "", failed: false });
  const bbox = useView((s) => s.picks.bbox);
  const patchPicks = useView((s) => s.patchPicks);

  // 지도 한 번 만들기
  useEffect(() => {
    if (!box.current || map.current) return;
    const m = L.map(box.current, {
      zoomControl: true,
      attributionControl: true,
      worldCopyJump: true,
    }).setView([36.5, 127.8], 6);
    L.tileLayer(TILES.url, {
      attribution: TILES.attribution,
      className: "acut-map-tile",
      maxZoom: 19,
    })
      .on("tileerror", () => setOffline(true))
      // Leaflet의 `load`는 모든 타일이 실패해도 끝났다는 뜻으로 온다.
      // 실제 타일 하나를 받은 경우에만 오프라인 표시를 푼다.
      .on("tileload", () => setOffline(false))
      .addTo(m);
    layer.current = L.layerGroup().addTo(m);
    m.on("moveend", () =>
      setView((v) => ({ zoom: m.getZoom(), tick: v.tick + 1 })),
    );
    map.current = m;
    // 크기가 나중에 잡히면 다시 잰다
    const ro = new ResizeObserver(() => m.invalidateSize());
    ro.observe(box.current);
    return () => {
      ro.disconnect();
      m.remove();
      map.current = null;
      layer.current = null;
    };
  }, []);

  // 지도는 고른 bbox 밖도 보여야 다른 데로 갈 수 있다. 나머지 조건의 전체
  // 장수와 경계를 따로 읽어, 조건을 바꿀 때마다 새 결과로 정확히 맞춘다.
  const precision = precisionForZoom(view.zoom);
  const filterKey = mapFilterKey(filter);
  const overviewRequestKey = `${filterKey}\u0000${retry}`;
  const cellsRequestKey = `${filterKey}\u0000${precision}\u0000${view.tick}\u0000${retry}`;
  useEffect(() => {
    let live = true;
    const f: Filter = { ...JSON.parse(filterKey), bbox: null };
    invoke<Overview>("map_overview", { filter: f })
      .then((overview) => {
        if (!live || !map.current) return;
        setOverviewState({
          key: overviewRequestKey,
          total: overview.total,
          failed: false,
        });
        if (!overview.bounds) {
          fittedFilter.current = filterKey;
          layer.current?.clearLayers();
          return;
        }
        // 같은 조건의 느린 중복 응답이 사용자가 확대한 지도를 다시 끌고
        // 가지 않게 한다. A → B → A처럼 조건이 실제로 바뀌면 다시 맞춘다.
        if (fittedFilter.current === filterKey) return;
        fittedFilter.current = filterKey;
        const [south, west, north, east] = overview.bounds;
        map.current.fitBounds(
          [
            [south, west],
            [north, east],
          ],
          { maxZoom: 12, animate: false },
        );
      })
      .catch(() => {
        if (!live) return;
        setOverviewState({
          key: overviewRequestKey,
          total: null,
          failed: true,
        });
      });
    return () => {
      live = false;
    };
  }, [filterKey, overviewRequestKey]);

  // 화면이 움직이거나 확대 단계가 바뀌면 현재 뷰포트의 칸만 묻는다. 전역
  // 상위 4,000개를 반복해서 읽지 않으므로 낮은 밀도의 지역도 이동하면 보인다.
  useEffect(() => {
    let live = true;
    const m = map.current;
    if (!m) return;
    const f: Filter = { ...JSON.parse(filterKey), bbox: null };
    const viewport = mapBbox(m);
    layer.current?.clearLayers();
    invoke<Cell[]>("map_cells", { filter: f, precision, viewport })
      .then((cells) => {
        if (!live || !map.current || !layer.current) return;
        setCellsState({ key: cellsRequestKey, failed: false });
        draw(layer.current, cells, precision, (c) => {
          const m = map.current!;
          if (isFinest(precision)) {
            patchPicks({
              bbox: bboxString(cellBounds(c.lat, c.lon, precision)),
            });
          } else {
            m.setView([c.lat, c.lon], Math.min(m.getZoom() + 3, 18));
          }
        });
      })
      .catch(() => {
        if (!live) return;
        layer.current?.clearLayers();
        setCellsState({ key: cellsRequestKey, failed: true });
      });
    return () => {
      live = false;
    };
  }, [cellsRequestKey, filterKey, precision, patchPicks]);

  // 고른 영역을 네모로
  useEffect(() => {
    const m = map.current;
    if (!m) return;
    picked.current?.remove();
    picked.current = null;
    const b = parseBbox(bbox);
    if (!b) return;
    picked.current = L.rectangle(
      [
        [b[0], b[1]],
        [b[2], b[3]],
      ],
      { color: "#00bafe", weight: 1.5, fillOpacity: 0.08, interactive: false },
    ).addTo(m);
  }, [bbox]);

  const useVisible = () => {
    const m = map.current;
    if (!m) return;
    const b = m.getBounds();
    patchPicks({
      bbox: bboxString(
        safeMapBbox(b.getSouth(), b.getWest(), b.getNorth(), b.getEast()),
      ),
    });
  };

  const count =
    overviewState.key === overviewRequestKey ? overviewState.total : null;
  const dataError =
    (overviewState.key === overviewRequestKey && overviewState.failed) ||
    (cellsState.key === cellsRequestKey && cellsState.failed);

  return (
    <div className="relative h-[42%] min-h-[180px] shrink-0 border-b border-line">
      <div ref={box} className="absolute inset-0 !bg-canvas" />
      <div className="absolute top-2 right-2 z-[500] flex items-center gap-1.5 text-[12.5px]">
        {count !== null && (
          <span className="px-2 h-7 rounded-md bg-raised/90 text-fg-dim flex items-center tabular-nums">
            위치 있는 사진 {count.toLocaleString()}장
          </span>
        )}
        <button
          onClick={useVisible}
          className="px-2 h-7 rounded-md bg-raised/90 text-fg hover:bg-hover"
          title="지금 보이는 영역의 사진만 아래에 남깁니다"
        >
          보이는 영역만
        </button>
        {bbox && (
          <button
            onClick={() => patchPicks({ bbox: null })}
            className="px-2 h-7 rounded-md bg-accent text-white hover:opacity-90"
            title="영역 조건을 풉니다"
          >
            영역 풀기
          </button>
        )}
      </div>
      {(offline || dataError) && (
        <div
          className="absolute left-2 bottom-6 z-[500] flex flex-col items-start gap-1 text-[12.5px]"
          aria-live="polite"
        >
          {offline && (
            <span className="px-2 py-1 rounded-md bg-raised/95 text-fg-dim">
              지도 타일을 못 받았습니다 — 오프라인? 왼쪽 목록으로 고르세요.
            </span>
          )}
          {dataError && (
            <span className="px-2 py-1 rounded-md bg-raised/95 text-fg-dim flex items-center gap-2">
              사진 위치를 불러오지 못했습니다.
              <button
                type="button"
                className="text-accent hover:underline"
                onClick={() => setRetry((n) => n + 1)}
              >
                다시 시도
              </button>
            </span>
          )}
        </div>
      )}
    </div>
  );
}

function mapBbox(m: L.Map): string {
  const b = m.getBounds();
  return bboxString(
    safeMapBbox(b.getSouth(), b.getWest(), b.getNorth(), b.getEast()),
  );
}

/** 정렬과 이미 고른 bbox는 지도 마커·경계에 영향을 주지 않는다. */
function mapFilterKey(filter: Filter): string {
  const value: Record<string, unknown> = { ...filter, bbox: null };
  delete value.sort;
  return JSON.stringify(value);
}

/** 칸을 마커로 — 썸네일 동그라미에 장수 */
function draw(
  layer: L.LayerGroup,
  cells: Cell[],
  precision: number,
  onClick: (c: Cell) => void,
) {
  layer.clearLayers();
  const fine = isFinest(precision);
  for (const c of cells) {
    const url = thumbUrl(c);
    const size = c.n >= 1000 ? 52 : c.n >= 100 ? 46 : c.n >= 10 ? 40 : 34;
    const html = `<div class="acut-pin" style="width:${size}px;height:${size}px;${
      url ? `background-image:url('${url}')` : ""
    }"><span>${c.n >= 10000 ? `${Math.round(c.n / 1000)}k` : c.n}</span></div>`;
    const icon = L.divIcon({
      html,
      className: "",
      iconSize: [size, size],
      iconAnchor: [size / 2, size / 2],
    });
    L.marker([c.lat, c.lon], {
      icon,
      title: pinLabel(c, fine),
    })
      .on("click", () => onClick(c))
      .addTo(layer);
  }
}
