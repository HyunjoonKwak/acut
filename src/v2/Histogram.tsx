import { useEffect, useRef, useState } from "react";
import { bins, polyline, type Bins } from "./histogramMath";

const W = 224;
const H = 64;
/** 나란히 보기의 좁은 띠 안에서 쓰는 높이 */
const H_COMPACT = 34;
/** 몇 픽셀만 세도 분포는 같다. 원본을 통째로 읽으면 넘길 때마다 멈칫한다. */
const SAMPLE = 220;

/**
 * 밝기 분포 — 인스펙터 안.
 *
 * 판정할 때 눈으로 잘 안 보이는 것이 둘 있다: 하이라이트가 날아간 것과
 * 그림자가 뭉갠 것. 작은 화면에서는 특히 그렇다.
 *
 * 이미 화면에 뜬 그림을 작은 캔버스에 옮겨 그려 픽셀을 읽는다. 원본을 다시
 * 열지 않는다.
 */
export default function Histogram({
  src,
  compact,
  getImage,
}: {
  src: string;
  /** 좁은 자리 — 제목을 빼고 낮게 그린다 */
  compact?: boolean;
  /** 이미 화면에 뜬 <img> — 있으면 그걸 읽어 원본을 두 번 디코드하지 않는다.
   *  그 img 에는 crossOrigin="anonymous" 가 있어야 캔버스가 오염되지 않는다 */
  getImage?: () => HTMLImageElement | null;
}) {
  const [h, setH] = useState<Bins | null>(null);
  const [err, setErr] = useState(false);
  const cvs = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    let live = true;
    setH(null);
    setErr(false);

    const draw = (img: HTMLImageElement) => {
      if (!live) return;
      try {
        const c = (cvs.current ??= document.createElement("canvas"));
        const iw = img.naturalWidth || img.width;
        const ih = img.naturalHeight || img.height;
        const r = iw / ih || 1;
        const w = Math.max(1, r >= 1 ? SAMPLE : Math.round(SAMPLE * r));
        const ht = Math.max(1, r >= 1 ? Math.round(SAMPLE / r) : SAMPLE);
        c.width = w;
        c.height = ht;
        const ctx = c.getContext("2d", { willReadFrequently: true });
        if (!ctx) return setErr(true);
        ctx.drawImage(img, 0, 0, w, ht);
        setH(bins(ctx.getImageData(0, 0, w, ht).data));
      } catch {
        // 오염된 캔버스거나 메모리가 모자란 경우. 없으면 없는 대로 둔다.
        setErr(true);
      }
    };

    // 1) 화면의 그림을 재사용 — 뷰어는 원본(수십 MB)을 이미 디코드했다
    const shown = getImage?.();
    if (shown && shown.src === src) {
      const onLoad = () => draw(shown);
      if (shown.complete && shown.naturalWidth > 0) draw(shown);
      else shown.addEventListener("load", onLoad, { once: true });
      return () => {
        live = false;
        shown.removeEventListener("load", onLoad);
      };
    }

    // 2) 없으면 따로 읽는다 (인스펙터의 썸네일처럼 작은 것)
    const img = new Image();
    // photo:// 는 웹뷰와 다른 오리진이다. 이게 없으면 캔버스가 오염돼
    // getImageData가 막힌다 (서버 쪽 CORS 헤더와 짝이다).
    img.crossOrigin = "anonymous";
    img.onload = () => draw(img);
    img.onerror = () => live && setErr(true);
    img.src = src;

    return () => {
      live = false;
      img.onload = null;
      img.onerror = null;
    };
  }, [src, getImage]);

  if (err) return null;
  const h1 = compact ? H_COMPACT : H;

  return (
    <>
      {!compact && (
        <div className="text-[10.5px] text-fg-mute uppercase tracking-wider mb-2">
          밝기 분포
        </div>
      )}
      <div
        className={compact ? "rounded bg-black/40" : "rounded bg-canvas p-1"}
      >
        <svg
          viewBox={`0 0 ${W} ${h1}`}
          className="w-full block"
          style={{ height: h1 }}
          role="img"
          aria-label="밝기 분포"
        >
          {/* 사분면 눈금 — 어디가 중간인지 알아야 치우침이 보인다 */}
          {[0.25, 0.5, 0.75].map((f) => (
            <line
              key={f}
              x1={W * f}
              y1={0}
              x2={W * f}
              y2={h1}
              stroke="var(--color-line)"
              strokeWidth={1}
            />
          ))}
          {h &&
            (
              [
                ["r", "#e5484d"],
                ["g", "#46a758"],
                ["b", "#3e8ae5"],
              ] as const
            ).map(([k, color]) => (
              <polyline
                key={k}
                points={polyline(h[k], h.peak, W, h1)}
                fill="none"
                stroke={color}
                strokeWidth={1}
                opacity={0.85}
                style={{ mixBlendMode: "screen" }}
              />
            ))}
        </svg>
      </div>
      {h && (h.clippedHighlight > 0.005 || h.clippedShadow > 0.005) && (
        <div className="mt-1.5 flex gap-3 text-[10.5px] tabular-nums">
          {h.clippedShadow > 0.005 && (
            <span className="text-fg-mute">
              뭉갠 그림자 {(h.clippedShadow * 100).toFixed(1)}%
            </span>
          )}
          {h.clippedHighlight > 0.005 && (
            <span className="text-drop">
              날아간 하이라이트 {(h.clippedHighlight * 100).toFixed(1)}%
            </span>
          )}
        </div>
      )}
    </>
  );
}
