import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes, fmtDateTime, fmtDuration } from "./format";
import TagEditor from "./TagEditor";
import Histogram from "./Histogram";
import RenameDialog from "./RenameDialog";
import CommentBox from "./CommentBox";

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
  durationMs: number | null;
  rating: number;
  cullingFlag: number;
  favorite: boolean;
  kind: number;
  comment: string | null;
};

const SOURCE_LABEL = ["EXIF", "파일명 추정", "파일시각 추정", "알 수 없음"];

export default function Viewer({
  ids,
  index,
  onIndex,
  onClose,
  onMark,
  fullScreen,
  onToggleFullScreen,
  kindOf,
  onRename,
}: {
  ids: number[];
  index: number;
  onIndex: (i: number) => void;
  onClose: () => void;
  onMark: (
    id: number,
    patch: { rating?: number; cullingFlag?: number; favorite?: boolean },
  ) => void;
  /// 켜면 창 전체를 덮는다. 끄면 콘텐츠 영역만 덮어 폴더 목록이 남는다.
  fullScreen: boolean;
  onToggleFullScreen: () => void;
  /** 사진인지 영상인지를 상세를 읽기 전에 알려 준다. 없으면 상세를 기다린다. */
  kindOf?: (id: number) => number | undefined;
  /** 이름을 바꾼다. 서버가 준 이름을 돌려준다. 실패는 던진다. */
  onRename?: (id: number, name: string) => Promise<string>;
}) {
  const [renaming, setRenaming] = useState(false);
  const id = ids[index];
  const player = useRef<HTMLVideoElement>(null);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [showInfo, setShowInfo] = useState(true);
  // 불러왔는지·실패했는지·확대했는지를 **어느 사진에 대해서**인지와 함께 둔다.
  // 사진이 바뀌면 id가 안 맞아 저절로 초기 상태가 된다 — 효과에서 셋을
  // 되돌릴 일이 없다.
  const [loadedId, setLoadedId] = useState<number | null>(null);
  const [failedId, setFailedId] = useState<number | null>(null);
  const loading = loadedId !== id && failedId !== id;
  const failed = failedId === id;
  /// 확대 — 배율과 보는 자리(0–1). 어느 사진 것인지와 함께 두어 넘기면 1로.
  const [view, setView] = useState<{
    id: number;
    scale: number;
    x: number;
    y: number;
  } | null>(null);
  const scale = view?.id === id ? view.scale : 1;
  const origin =
    view?.id === id ? { x: view.x, y: view.y } : { x: 0.5, y: 0.5 };
  const zoom = scale > 1;
  const resetZoom = useCallback(() => setView(null), []);
  const zoomTo = useCallback(
    (s: number, x: number, y: number) =>
      setView(s <= 1 ? null : { id, scale: Math.min(8, s), x, y }),
    [id],
  );
  /// 커서 자리를 기준으로 휠 확대. 나란히 보기와 같은 손맛.
  const onWheel = (e: React.WheelEvent<HTMLDivElement>) => {
    if (isVideo) return;
    const r = e.currentTarget.getBoundingClientRect();
    const fx = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    const fy = Math.min(1, Math.max(0, (e.clientY - r.top) / r.height));
    zoomTo(scale * (e.deltaY < 0 ? 1.15 : 1 / 1.15), fx, fy);
  };
  /// 확대한 상태에서 끌면 옮겨 다닌다
  const onMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.buttons !== 1 || !zoom) return;
    const r = e.currentTarget.getBoundingClientRect();
    setView((v) =>
      v && v.id === id
        ? {
            ...v,
            x: Math.min(1, Math.max(0, v.x - e.movementX / r.width / v.scale)),
            y: Math.min(1, Math.max(0, v.y - e.movementY / r.height / v.scale)),
          }
        : v,
    );
  };

  /// 슬라이드쇼 — 3초마다 다음 장. 끝에 닿거나 아무 키나 누르면 멈춘다.
  const [playing, setPlaying] = useState(false);
  useEffect(() => {
    if (!playing) return;
    const t = setInterval(() => {
      if (index + 1 < ids.length) onIndex(index + 1);
      else setPlaying(false);
    }, 3000);
    return () => clearInterval(t);
  }, [playing, index, ids.length, onIndex]);

  useEffect(() => {
    if (id == null) return;
    invoke<Detail>("file_detail", { id })
      .then(setDetail)
      .catch(() => setDetail(null));
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
      // 슬라이드쇼는 무슨 키든 누르면 멈춘다 — S만 켜고 끄는 스위치
      if (e.key === "s") {
        setPlaying((p) => !p);
        return;
      }
      setPlaying(false);
      switch (e.key) {
        case "Escape":
          if (zoom) resetZoom();
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
        case " ": {
          e.preventDefault();
          // 영상은 재생·정지, 사진은 확대
          const v = player.current;
          if (v) {
            if (v.paused) v.play().catch(() => {});
            else v.pause();
          } else if (zoom) resetZoom();
          else zoomTo(2, 0.5, 0.5);
          break;
        }
        case "i":
          setShowInfo((s) => !s);
          break;
        case "\\":
          onToggleFullScreen();
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
  }, [
    step,
    onClose,
    zoom,
    resetZoom,
    zoomTo,
    mark,
    detail?.favorite,
    onToggleFullScreen,
  ]);

  if (id == null) return null;
  const src = `photo://localhost/${id}`;
  // 상세가 오기 전에도 알 수 있으면 그걸 쓴다 — 정지 프레임이 한 번
  // 그려졌다가 영상으로 바뀌면 깜빡인다.
  const isVideo = (kindOf?.(id) ?? detail?.kind) === 1;

  return (
    <div
      className={`${
        fullScreen ? "fixed inset-0 z-50" : "absolute inset-0 z-30"
      } bg-canvas flex flex-col`}
    >
      {/* 상단 */}
      <div className="h-11 shrink-0 flex items-center gap-3 px-4 bg-raised/95 border-b border-line text-[12.5px]">
        <button
          onClick={() => onRename && setRenaming(true)}
          title={onRename ? "이름 바꾸기" : undefined}
          className={`text-fg truncate max-w-[40%] text-left ${onRename ? "hover:underline" : ""}`}
        >
          {detail?.name ?? "…"}
        </button>
        <span className="text-fg-mute tabular-nums">
          {index + 1} / {ids.length}
        </span>
        {detail && detail.cullingFlag === 1 && (
          <span className="px-2 py-0.5 rounded bg-keep text-keep-fg text-[11px] font-bold">
            ★ 남김
          </span>
        )}
        {detail && detail.cullingFlag === 2 && (
          <span className="px-2 py-0.5 rounded bg-drop text-drop-fg text-[11px] font-bold">
            ✕ 제외
          </span>
        )}
        {detail && detail.rating > 0 && (
          <span className="text-keep tracking-tight">
            {"★".repeat(detail.rating)}
          </span>
        )}
        {detail?.favorite && <span className="text-drop">♥</span>}
        <div className="flex-1" />
        {zoom && (
          <button onClick={resetZoom} className="text-fg-dim px-2 tabular-nums">
            {scale.toFixed(1)}× 되돌리기
          </button>
        )}
        <button
          onClick={() => setPlaying((p) => !p)}
          className={`px-2 ${playing ? "text-accent" : "text-fg-dim"}`}
        >
          {playing ? "멈춤" : "슬라이드쇼"}{" "}
          <span className="text-[10px] font-mono">S</span>
        </button>
        <button
          onClick={() => setShowInfo((s) => !s)}
          className="text-fg-dim px-2"
        >
          정보 <span className="text-[10px] font-mono">I</span>
        </button>
        <button
          onClick={onToggleFullScreen}
          className={`px-2 ${fullScreen ? "text-accent" : "text-fg-dim"}`}
        >
          {fullScreen ? "창 안에서" : "전체화면"}{" "}
          <span className="text-[10px] font-mono">\</span>
        </button>
        <button onClick={onClose} className="text-fg-dim px-2">
          닫기 <span className="text-[10px] font-mono">Esc</span>
        </button>
      </div>

      <div className="flex-1 flex min-h-0">
        {/* 사진 */}
        <div
          className="flex-1 relative min-w-0 overflow-hidden flex items-center justify-center touch-none"
          onWheel={onWheel}
          onPointerMove={onMove}
          onClick={(e) => {
            if (isVideo) return;
            if (zoom) return resetZoom();
            const r = e.currentTarget.getBoundingClientRect();
            zoomTo(
              2,
              (e.clientX - r.left) / r.width,
              (e.clientY - r.top) / r.height,
            );
          }}
        >
          {loading && (
            <div className="absolute inset-0 flex items-center justify-center text-fg-faint text-[13px] pointer-events-none">
              불러오는 중…
            </div>
          )}
          {failed ? (
            <div className="flex flex-col items-center gap-2 text-fg-mute text-[13px]">
              {isVideo
                ? "이 앱이 틀 수 없는 영상입니다"
                : "읽을 수 없는 파일입니다"}
              <span className="text-[11.5px] text-fg-faint">
                {detail?.name}
              </span>
              {/* WebKit이 H.264·HEVC·VP9까지만 튼다. 나머지(ProRes 등)는 QuickTime에 맡긴다. */}
              {isVideo && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    invoke("open_in_default_app", { id }).catch(() => {});
                  }}
                  className="mt-1 h-control px-3 rounded-md bg-raised text-fg text-[12px] hover:bg-hover"
                >
                  QuickTime으로 열기
                </button>
              )}
            </div>
          ) : isVideo ? (
            /* 영상 — 타일에서 쓰던 video:// 그대로. QuickLook 프레임을 포스터로
               깔아 두어 첫 프레임이 오기 전에도 빈 화면이 아니다. */
            <video
              ref={player}
              key={id}
              src={`video://localhost/${id}`}
              poster={src}
              controls
              autoPlay
              playsInline
              onLoadedData={() => setLoadedId(id)}
              onError={() => setFailedId(id)}
              onClick={(e) => e.stopPropagation()}
              className="max-w-full max-h-full"
              style={{
                opacity: loading ? 0.001 : 1,
                transition: "opacity .12s",
              }}
            />
          ) : (
            <img
              src={src}
              onLoad={() => setLoadedId(id)}
              onError={() => setFailedId(id)}
              draggable={false}
              className={`max-w-full max-h-full object-contain select-none ${
                zoom ? "cursor-grab" : "cursor-zoom-in"
              }`}
              style={{
                opacity: loading ? 0 : 1,
                transform: `scale(${scale})`,
                transformOrigin: `${origin.x * 100}% ${origin.y * 100}%`,
                transition: "opacity .12s",
              }}
            />
          )}

          {/* 영상 배지 — 재생기가 아직 안 떴을 때만. 떴으면 컨트롤이 말해 준다 */}
          {isVideo && !failed && loading && (
            <span className="absolute bottom-4 left-1/2 -translate-x-1/2 px-2.5 py-1 rounded-full bg-black/60 text-fg text-[12px] pointer-events-none">
              ▶ 영상
              {detail?.durationMs ? ` · ${fmtDuration(detail.durationMs)}` : ""}
            </span>
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
          <aside className="w-64 shrink-0 bg-raised border-l border-line p-4 overflow-y-auto text-[12px]">
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
            {detail.durationMs ? (
              <Row k="길이" v={fmtDuration(detail.durationMs)} />
            ) : null}
            <Row k="폴더" v={detail.folder || "/"} />
            <CommentBox
              key={id}
              id={id}
              initial={detail.comment ?? ""}
              onSaved={(c) => setDetail((d) => (d ? { ...d, comment: c } : d))}
            />

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
            <div className="text-[10.5px] text-fg-mute uppercase tracking-wider mb-2">
              판정
            </div>
            <div className="flex gap-1 mb-2">
              {[1, 2, 3, 4, 5].map((n) => (
                <button
                  key={n}
                  onClick={() => mark({ rating: detail.rating === n ? 0 : n })}
                  className={`w-6 h-6 rounded ${
                    detail.rating >= n ? "text-keep" : "text-fg-faint"
                  }`}
                >
                  ★
                </button>
              ))}
            </div>
            <div className="flex gap-1.5">
              <button
                onClick={() =>
                  mark({ cullingFlag: detail.cullingFlag === 1 ? 0 : 1 })
                }
                className={`flex-1 h-7 rounded text-[11.5px] font-semibold ${
                  detail.cullingFlag === 1
                    ? "bg-keep text-keep-fg"
                    : "text-fg-dim ring-1 ring-line"
                }`}
              >
                남김 <span className="font-mono text-[10px]">P</span>
              </button>
              <button
                onClick={() =>
                  mark({ cullingFlag: detail.cullingFlag === 2 ? 0 : 2 })
                }
                className={`flex-1 h-7 rounded text-[11.5px] font-semibold ${
                  detail.cullingFlag === 2
                    ? "bg-drop text-drop-fg"
                    : "text-fg-dim ring-1 ring-line"
                }`}
              >
                제외 <span className="font-mono text-[10px]">X</span>
              </button>
            </div>

            {/* 영상은 대표 프레임 한 장이라 분포가 큰 뜻이 없다 */}
            {!isVideo && !failed && (
              <>
                <Sep />
                <Histogram src={src} />
              </>
            )}

            <Sep />
            <TagEditor id={id} />

            <Sep />
            <div className="text-[10.5px] text-fg-mute leading-relaxed">
              <b className="text-fg-mute">단축키</b>
              <br />← → 이동 · Space 확대 · 0–5 별점
              <br />P 남김 · X 제외 · F 즐겨찾기
              <br />I 정보 · \ 전체화면 · S 슬라이드쇼
              <br />휠 확대 · 끌어서 이동
            </div>
          </aside>
        )}
      </div>
      {renaming && detail && onRename && (
        <RenameDialog
          name={detail.name}
          onSubmit={async (n) => {
            const next = await onRename(id, n);
            setDetail((d) => (d ? { ...d, name: next } : d));
          }}
          onClose={() => setRenaming(false)}
        />
      )}
    </div>
  );
}

function Row({ k, v, dim }: { k: string; v: string; dim?: boolean }) {
  return (
    <div className="flex justify-between gap-3 py-[3px]">
      <span className="text-fg-mute shrink-0">{k}</span>
      <span
        className={`text-right truncate font-mono text-[11px] ${dim ? "text-keep" : "text-fg-dim"}`}
        title={v}
      >
        {v}
      </span>
    </div>
  );
}
function Sep() {
  return <div className="h-px bg-line my-3" />;
}
