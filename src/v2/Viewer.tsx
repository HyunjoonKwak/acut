import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Detail = {
  name: string;
  folder: string;
  size: number;
  takenAt: number;
  takenAtSource: number;
  width: number | null;
  height: number | null;
  camMake: string | null;
  camModel: string | null;
  lens: string | null;
  iso: number | null;
  aperture: number | null;
  shutter: string | null;
  focalMm: number | null;
  rating: number;
  cullingFlag: number;
  favorite: boolean;
  kind: number;
};

const SOURCE_LABEL = ["EXIF", "파일명 추정", "파일시각 추정", "알 수 없음"];

const fmtBytes = (n: number) => {
  const u = ["B", "KB", "MB", "GB"];
  let v = n,
    i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${i === 0 ? v : v.toFixed(1)} ${u[i]}`;
};
const fmtDateTime = (ts: number) =>
  new Date(ts * 1000).toLocaleString("ko-KR", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });

export default function Viewer({
  ids,
  index,
  onIndex,
  onClose,
  onMark,
}: {
  ids: number[];
  index: number;
  onIndex: (i: number) => void;
  onClose: () => void;
  onMark: (id: number, patch: { rating?: number; cullingFlag?: number; favorite?: boolean }) => void;
}) {
  const id = ids[index];
  const [detail, setDetail] = useState<Detail | null>(null);
  const [zoom, setZoom] = useState(false);
  const [showInfo, setShowInfo] = useState(true);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (id == null) return;
    setLoading(true);
    setFailed(false);
    setZoom(false);
    invoke<Detail>("file_detail", { id }).then(setDetail).catch(() => setDetail(null));
  }, [id]);

  const step = useCallback(
    (d: number) => {
      const next = index + d;
      if (next >= 0 && next < ids.length) onIndex(next);
    },
    [index, ids.length, onIndex],
  );

  const mark = useCallback(
    (patch: Parameters<typeof onMark>[1]) => {
      if (id == null) return;
      onMark(id, patch);
      setDetail((d) =>
        d
          ? {
              ...d,
              rating: patch.rating ?? d.rating,
              cullingFlag: patch.cullingFlag ?? d.cullingFlag,
              favorite: patch.favorite ?? d.favorite,
            }
          : d,
      );
    },
    [id, onMark],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      switch (e.key) {
        case "Escape":
          if (zoom) setZoom(false);
          else onClose();
          break;
        case "ArrowRight":
        case "j":
          step(1);
          break;
        case "ArrowLeft":
        case "k":
          step(-1);
          break;
        case " ":
          e.preventDefault();
          setZoom((z) => !z);
          break;
        case "i":
          setShowInfo((s) => !s);
          break;
        case "x":
          mark({ cullingFlag: 2 });
          break;
        case "p":
          mark({ cullingFlag: 1 });
          break;
        case "f":
          mark({ favorite: !detail?.favorite });
          break;
        default:
          if (/^[0-5]$/.test(e.key)) mark({ rating: +e.key });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [step, onClose, zoom, mark, detail?.favorite]);

  if (id == null) return null;
  const src = `photo://localhost/${id}`;
  const isVideo = detail?.kind === 1;

  return (
    <div className="fixed inset-0 z-50 bg-[#0B0E0F] flex flex-col">
      {/* 상단 */}
      <div className="h-11 shrink-0 flex items-center gap-3 px-4 bg-[#141A1B]/95 border-b border-[#212A2C] text-[12.5px]">
        <span className="text-[#EAEFEF] truncate max-w-[40%]">{detail?.name ?? "…"}</span>
        <span className="text-[#6D7B7E] tabular-nums">
          {index + 1} / {ids.length}
        </span>
        {detail && detail.cullingFlag === 1 && (
          <span className="px-2 py-0.5 rounded bg-[#F0B429] text-[#231A00] text-[11px] font-bold">
            ★ 남김
          </span>
        )}
        {detail && detail.cullingFlag === 2 && (
          <span className="px-2 py-0.5 rounded bg-[#E2685C] text-[#2A0D09] text-[11px] font-bold">
            ✕ 제외
          </span>
        )}
        {detail && detail.rating > 0 && (
          <span className="text-[#F0B429] tracking-tight">{"★".repeat(detail.rating)}</span>
        )}
        {detail?.favorite && <span className="text-[#E2685C]">♥</span>}
        <div className="flex-1" />
        <button onClick={() => setShowInfo((s) => !s)} className="text-[#8D9A9C] px-2">
          정보 <span className="text-[10px] font-mono">I</span>
        </button>
        <button onClick={onClose} className="text-[#8D9A9C] px-2">
          닫기 <span className="text-[10px] font-mono">Esc</span>
        </button>
      </div>

      <div className="flex-1 flex min-h-0">
        {/* 사진 */}
        <div
          className={`flex-1 relative min-w-0 ${zoom ? "overflow-auto" : "overflow-hidden flex items-center justify-center"}`}
          onClick={() => setZoom((z) => !z)}
        >
          {loading && !isVideo && (
            <div className="absolute inset-0 flex items-center justify-center text-[#4E5A5C] text-[13px] pointer-events-none">
              불러오는 중…
            </div>
          )}
          {/* 영상은 아직 ImageIO로 한 장을 뽑지 못한다 — AVFoundation이 붙기 전까지는 안내만 */}
          {isVideo ? (
            <div className="flex flex-col items-center gap-2 text-[#5F6C6E] text-[13px]">
              <span className="text-3xl">▶</span>
              영상은 아직 크게 볼 수 없습니다
              <span className="text-[11.5px] text-[#4E5A5C]">{detail?.name}</span>
            </div>
          ) : failed ? (
            <div className="flex flex-col items-center gap-2 text-[#5F6C6E] text-[13px]">
              읽을 수 없는 파일입니다
              <span className="text-[11.5px] text-[#4E5A5C]">{detail?.name}</span>
            </div>
          ) : (
            <img
              src={src}
              onLoad={() => setLoading(false)}
              onError={() => {
                setLoading(false);
                setFailed(true);
              }}
              className={
                zoom
                  ? "max-w-none cursor-zoom-out"
                  : "max-w-full max-h-full object-contain cursor-zoom-in"
              }
              style={{ opacity: loading ? 0 : 1, transition: "opacity .12s" }}
            />
          )}

          {/* 좌우 */}
          {index > 0 && (
            <button
              onClick={(e) => {
                e.stopPropagation();
                step(-1);
              }}
              className="absolute left-3 top-1/2 -translate-y-1/2 w-10 h-10 rounded-full bg-black/45 text-white text-lg"
            >
              ‹
            </button>
          )}
          {index < ids.length - 1 && (
            <button
              onClick={(e) => {
                e.stopPropagation();
                step(1);
              }}
              className="absolute right-3 top-1/2 -translate-y-1/2 w-10 h-10 rounded-full bg-black/45 text-white text-lg"
            >
              ›
            </button>
          )}
        </div>

        {/* 인스펙터 */}
        {showInfo && detail && (
          <aside className="w-64 shrink-0 bg-[#141A1B] border-l border-[#212A2C] p-4 overflow-y-auto text-[12px]">
            <Row k="촬영" v={fmtDateTime(detail.takenAt)} />
            <Row
              k="날짜 출처"
              v={SOURCE_LABEL[detail.takenAtSource] ?? "?"}
              dim={detail.takenAtSource !== 0}
            />
            <Row
              k="크기"
              v={
                detail.width && detail.height
                  ? `${detail.width} × ${detail.height}`
                  : "—"
              }
            />
            <Row k="용량" v={fmtBytes(detail.size)} />
            <Row k="폴더" v={detail.folder || "/"} />

            {(detail.camModel || detail.lens) && (
              <>
                <Sep />
                <Row k="카메라" v={detail.camModel ?? "—"} />
                <Row k="렌즈" v={detail.lens ?? "—"} />
                <Row
                  k="설정"
                  v={
                    [
                      detail.shutter,
                      detail.aperture ? `f${detail.aperture}` : null,
                      detail.iso ? `ISO ${detail.iso}` : null,
                      detail.focalMm ? `${detail.focalMm}mm` : null,
                    ]
                      .filter(Boolean)
                      .join(" · ") || "—"
                  }
                />
              </>
            )}

            <Sep />
            <div className="text-[10.5px] text-[#5F6C6E] uppercase tracking-wider mb-2">
              판정
            </div>
            <div className="flex gap-1 mb-2">
              {[1, 2, 3, 4, 5].map((n) => (
                <button
                  key={n}
                  onClick={() => mark({ rating: detail.rating === n ? 0 : n })}
                  className={`w-6 h-6 rounded ${
                    detail.rating >= n ? "text-[#F0B429]" : "text-[#3A4547]"
                  }`}
                >
                  ★
                </button>
              ))}
            </div>
            <div className="flex gap-1.5">
              <button
                onClick={() => mark({ cullingFlag: detail.cullingFlag === 1 ? 0 : 1 })}
                className={`flex-1 h-7 rounded text-[11.5px] font-semibold ${
                  detail.cullingFlag === 1
                    ? "bg-[#F0B429] text-[#231A00]"
                    : "text-[#8D9A9C] ring-1 ring-[#2E383A]"
                }`}
              >
                남김 <span className="font-mono text-[10px]">P</span>
              </button>
              <button
                onClick={() => mark({ cullingFlag: detail.cullingFlag === 2 ? 0 : 2 })}
                className={`flex-1 h-7 rounded text-[11.5px] font-semibold ${
                  detail.cullingFlag === 2
                    ? "bg-[#E2685C] text-[#2A0D09]"
                    : "text-[#8D9A9C] ring-1 ring-[#2E383A]"
                }`}
              >
                제외 <span className="font-mono text-[10px]">X</span>
              </button>
            </div>

            <Sep />
            <div className="text-[10.5px] text-[#5F6C6E] leading-relaxed">
              <b className="text-[#7C8A8D]">단축키</b>
              <br />← → 이동 · Space 확대 · 0–5 별점
              <br />P 남김 · X 제외 · F 즐겨찾기 · I 정보
            </div>
          </aside>
        )}
      </div>
    </div>
  );
}

function Row({ k, v, dim }: { k: string; v: string; dim?: boolean }) {
  return (
    <div className="flex justify-between gap-3 py-[3px]">
      <span className="text-[#5F6C6E] shrink-0">{k}</span>
      <span
        className={`text-right truncate font-mono text-[11px] ${dim ? "text-[#E0955A]" : "text-[#A3B2B4]"}`}
        title={v}
      >
        {v}
      </span>
    </div>
  );
}
function Sep() {
  return <div className="h-px bg-[#212A2C] my-3" />;
}
