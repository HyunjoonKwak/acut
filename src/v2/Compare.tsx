import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes, megapixels } from "./format";
import Histogram from "./Histogram";
import { toast } from "./toastStore";

type Detail = {
  name: string;
  size: number;
  width: number | null;
  height: number | null;
  iso: number | null;
  aperture: number | null;
  shutter: string | null;
  focalMm: number | null;
  rating: number;
  cullingFlag: number;
  favorite: boolean;
};

export type Mark = {
  rating?: number;
  cullingFlag?: number;
  favorite?: boolean;
};

/**
 * 나란히 놓고 보기 — 비슷한 것 중 하나를 고를 때.
 *
 * 연달아 찍은 다섯 장 중 어느 것이 제일 나은지는 번갈아 넘겨서는 알기
 * 어렵다. 눈이 앞 장을 기억하지 못한다. 나란히 놓으면 바로 보인다.
 *
 * **확대·이동은 함께 움직인다.** 같은 자리를 같은 배율로 봐야 초점이
 * 맞았는지 비교가 된다. 따로 놀면 비교가 아니라 두 장 보기다.
 */
export default function Compare({
  ids,
  onMark,
  onClose,
}: {
  /** 나란히 놓을 사진들 (2–4장) */
  ids: number[];
  onMark: (id: number, patch: Mark) => Promise<void>;
  onClose: () => void;
}) {
  const [details, setDetails] = useState<Map<number, Detail>>(new Map());
  /// 함께 움직이는 확대 — 1이면 칸에 맞춘 상태
  const [zoom, setZoom] = useState(1);
  /// 확대했을 때 보는 자리 (0–1). 가운데가 0.5, 0.5
  const [at, setAt] = useState({ x: 0.5, y: 0.5 });
  /// 키보드가 겨누는 칸
  const [focus, setFocus] = useState(0);
  const [showInfo, setShowInfo] = useState(true);

  useEffect(() => {
    let live = true;
    Promise.all(
      ids.map((id) =>
        invoke<Detail>("file_detail", { id })
          .then((d) => [id, d] as const)
          .catch(() => null),
      ),
    ).then((rs) => {
      if (live) setDetails(new Map(rs.filter((r) => r !== null)));
    });
    return () => {
      live = false;
    };
  }, [ids]);

  const mark = useCallback(
    (id: number, patch: Mark) => {
      void onMark(id, patch)
        .then(() => {
          setDetails((prev) => {
            const d = prev.get(id);
            if (!d) return prev;
            const next = new Map(prev);
            next.set(id, {
              ...d,
              rating: patch.rating ?? d.rating,
              cullingFlag: patch.cullingFlag ?? d.cullingFlag,
              favorite: patch.favorite ?? d.favorite,
            });
            return next;
          });
        })
        .catch((e) => {
          toast(`판정을 저장하지 못했습니다 — ${String(e)}`, "drop");
        });
    },
    [onMark],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const id = ids[focus];
      switch (e.key) {
        case "Escape":
          onClose();
          return;
        case "ArrowRight":
          setFocus((f) => Math.min(ids.length - 1, f + 1));
          return;
        case "ArrowLeft":
          setFocus((f) => Math.max(0, f - 1));
          return;
        case "i":
          setShowInfo((s) => !s);
          return;
        case "0":
          setZoom(1);
          setAt({ x: 0.5, y: 0.5 });
          return;
        case "p":
          if (id != null) mark(id, { cullingFlag: 1 });
          return;
        case "x":
          if (id != null) mark(id, { cullingFlag: 2 });
          return;
        case "f":
          if (id != null) mark(id, { favorite: !details.get(id)?.favorite });
          return;
      }
      // 1–4는 칸을 겨눈다. 별점은 여기서 안 준다 — 칸 번호와 겹친다.
      if (/^[1-9]$/.test(e.key)) {
        const n = +e.key - 1;
        if (n < ids.length) setFocus(n);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [ids, focus, mark, onClose, details]);

  /// 휠로 함께 확대. 커서가 있는 자리를 기준으로 삼는다.
  const onWheel = (e: React.WheelEvent<HTMLDivElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    setAt({
      x: Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)),
      y: Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)),
    });
    setZoom((z) =>
      Math.min(8, Math.max(1, z * (e.deltaY < 0 ? 1.15 : 1 / 1.15))),
    );
  };

  /// 확대한 상태에서 끌면 함께 움직인다
  const onMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.buttons !== 1 || zoom === 1) return;
    const r = e.currentTarget.getBoundingClientRect();
    setAt((p) => ({
      x: Math.min(1, Math.max(0, p.x - e.movementX / r.width / zoom)),
      y: Math.min(1, Math.max(0, p.y - e.movementY / r.height / zoom)),
    }));
  };

  return (
    <div className="fixed inset-0 z-[60] bg-canvas flex flex-col">
      <div className="h-11 shrink-0 flex items-center gap-3 px-4 bg-raised/95 border-b border-line text-[13.5px]">
        <span className="text-fg font-semibold">나란히 보기</span>
        <span className="text-fg-mute tabular-nums">{ids.length}장</span>
        <span className="text-fg-faint text-[12.5px]">
          휠로 확대 · 끌어서 이동 · 함께 움직입니다
        </span>
        <div className="flex-1" />
        {zoom > 1 && (
          <button
            onClick={() => {
              setZoom(1);
              setAt({ x: 0.5, y: 0.5 });
            }}
            className="text-fg-dim px-2 tabular-nums"
          >
            {zoom.toFixed(1)}× 되돌리기{" "}
            <span className="text-[11px] font-mono">0</span>
          </button>
        )}
        <button
          onClick={() => setShowInfo((s) => !s)}
          className={`px-2 ${showInfo ? "text-accent" : "text-fg-dim"}`}
        >
          정보 <span className="text-[11px] font-mono">I</span>
        </button>
        <button onClick={onClose} className="text-fg-dim px-2">
          닫기 <span className="text-[11px] font-mono">Esc</span>
        </button>
      </div>

      <div
        className="flex-1 min-h-0 grid gap-1 p-1"
        style={{
          // 2장은 좌우로, 3–4장은 2×2로. 세로로 길게 늘어놓으면 비교가 안 된다.
          gridTemplateColumns: ids.length <= 2 ? "1fr 1fr" : "1fr 1fr",
          gridTemplateRows: ids.length <= 2 ? "1fr" : "1fr 1fr",
        }}
      >
        {ids.map((id, i) => {
          const d = details.get(id);
          const on = i === focus;
          return (
            <div
              key={id}
              onPointerDown={() => setFocus(i)}
              onWheel={onWheel}
              onPointerMove={onMove}
              className="relative min-w-0 min-h-0 overflow-hidden rounded bg-black/25 touch-none"
              style={{
                outline: on ? "2px solid var(--color-accent)" : undefined,
                outlineOffset: -2,
                cursor: zoom > 1 ? "grab" : "default",
              }}
            >
              <img
                src={`photo://localhost/${id}`}
                draggable={false}
                className="absolute inset-0 w-full h-full object-contain select-none"
                style={{
                  transform: `scale(${zoom})`,
                  transformOrigin: `${at.x * 100}% ${at.y * 100}%`,
                }}
              />

              {/* 칸 번호 — 키보드로 겨눌 때 쓴다 */}
              <span
                className={`absolute top-1.5 left-1.5 w-5 h-5 rounded text-[12px] font-bold
                  flex items-center justify-center ${
                    on ? "bg-accent text-accent-fg" : "bg-black/55 text-fg-dim"
                  }`}
              >
                {i + 1}
              </span>

              {d && (
                <div className="absolute bottom-0 inset-x-0 bg-black/60 px-2 py-1.5">
                  <div className="flex items-center gap-2 min-w-0">
                    <span className="text-[12.5px] text-fg truncate flex-1">
                      {d.name}
                    </span>
                    {d.cullingFlag === 1 && (
                      <span className="px-1.5 rounded bg-keep text-keep-fg text-[11px] font-bold shrink-0">
                        ★ 남김
                      </span>
                    )}
                    {d.cullingFlag === 2 && (
                      <span className="px-1.5 rounded bg-drop text-drop-fg text-[11px] font-bold shrink-0">
                        ✕ 제외
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-2 text-[11.5px] text-fg-mute tabular-nums mt-0.5">
                    <span>{fmtBytes(d.size)}</span>
                    {d.width && d.height && (
                      <span>
                        {d.width}×{d.height} {megapixels(d.width, d.height)}
                      </span>
                    )}
                    <span className="text-fg-faint">
                      {[
                        d.focalMm ? `${d.focalMm}mm` : null,
                        d.shutter,
                        d.aperture ? `f${d.aperture}` : null,
                        d.iso ? `ISO ${d.iso}` : null,
                      ]
                        .filter(Boolean)
                        .join(" · ")}
                    </span>
                    <div className="flex-1" />
                    <button
                      onClick={() =>
                        mark(id, { cullingFlag: d.cullingFlag === 1 ? 0 : 1 })
                      }
                      className={`px-1.5 h-5 rounded text-[11.5px] font-semibold ${
                        d.cullingFlag === 1
                          ? "bg-keep text-keep-fg"
                          : "text-fg-dim ring-1 ring-line"
                      }`}
                    >
                      남김
                    </button>
                    <button
                      onClick={() =>
                        mark(id, { cullingFlag: d.cullingFlag === 2 ? 0 : 2 })
                      }
                      className={`px-1.5 h-5 rounded text-[11.5px] font-semibold ${
                        d.cullingFlag === 2
                          ? "bg-drop text-drop-fg"
                          : "text-fg-dim ring-1 ring-line"
                      }`}
                    >
                      제외
                    </button>
                  </div>

                  {/* 초점이 간 칸만 분포를 그린다 — 넷을 동시에 세면 넘길 때마다 멈칫한다 */}
                  {showInfo && on && (
                    <div className="mt-1.5">
                      <Histogram src={`photo://localhost/${id}`} compact />
                    </div>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
