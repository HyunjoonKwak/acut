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
} from "./mapMath";

type Cell = {
  lat: number;
  lon: number;
  n: number;
  library_id: number | null;
  thumb: string | null;
};

const thumbUrl = (c: Cell) => (c.thumb && c.library_id !== null ? thumbUrlOf(c.library_id, c.thumb) : null);

/** 타일은 온라인 — 사용자 결정(2026-08-27). 어두운 바탕이 앱과 맞는다. */
const TILES = "https://{s}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}{r}.png";
const ATTRIBUTION = "© OpenStreetMap contributors © CARTO";

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
  const [view, setView] = useState<{ zoom: number; tick: number }>({
    zoom: 3,
    tick: 0,
  });
  const [offline, setOffline] = useState(false);
  const [count, setCount] = useState<number | null>(null);
  const bbox = useView((s) => s.picks.bbox);
  const patchPicks = useView((s) => s.patchPicks);
  const fitted = useRef(false);

  // 지도 한 번 만들기
  useEffect(() => {
    if (!box.current || map.current) return;
    const m = L.map(box.current, {
      zoomControl: true,
      attributionControl: true,
      worldCopyJump: true,
    }).setView([36.5, 127.8], 6);
    L.tileLayer(TILES, {
      attribution: ATTRIBUTION,
      subdomains: "abcd",
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

  // 조건이나 화면이 바뀌면 칸을 다시 묻는다. 지도는 bbox 말고 나머지 조건만
  // 본다 — 고른 영역 밖도 보여야 다른 데로 갈 수 있다.
  const precision = precisionForZoom(view.zoom);
  const filterKey = JSON.stringify({ ...filter, bbox: null });
  useEffect(() => {
    let live = true;
    const f: Filter = { ...JSON.parse(filterKey), bbox: null };
    invoke<Cell[]>("map_cells", { filter: f, precision })
      .then((cells) => {
        if (!live || !map.current || !layer.current) return;
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
        setCount(cells.reduce((a, c) => a + c.n, 0));
        // 처음엔 사진이 있는 곳으로 — 세상 지도 한가운데가 아니라
        if (!fitted.current && cells.length > 0) {
          fitted.current = true;
          const b = L.latLngBounds(cells.map((c) => [c.lat, c.lon]));
          map.current.fitBounds(b.pad(0.2), { maxZoom: 12, animate: false });
        }
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [filterKey, precision, view.tick, patchPicks]);

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
      bbox: bboxString([b.getSouth(), b.getWest(), b.getNorth(), b.getEast()]),
    });
  };

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
      {offline && (
        <div className="absolute left-2 bottom-6 z-[500] px-2 py-1 rounded-md bg-raised/95 text-[12.5px] text-fg-dim">
          지도 타일을 못 받았습니다 — 오프라인? 왼쪽 목록으로 고르세요.
        </div>
      )}
    </div>
  );
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
      title: fine
        ? `${c.n}장 — 누르면 이 자리의 사진만`
        : `${c.n}장 — 누르면 확대`,
    })
      .on("click", () => onClick(c))
      .addTo(layer);
  }
}
