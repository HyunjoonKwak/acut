import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Suggestion = { title: string; why: string; score: number };
type Outcome = {
  batch_id: number;
  moved: number;
  failed: number;
  bytes: number;
  first_error: string | null;
};

/**
 * 정리 — 고른 사진에 이벤트 이름을 붙여 폴더로 옮긴다.
 *
 * 이름을 처음부터 타이핑하게 두면 정리를 안 하게 된다. 이미 쓴 폴더명이
 * 가장 좋은 재료라 제안을 먼저 보여 주고, 고치고 싶으면 고치게 한다.
 */
export default function Organize({
  ids,
  libraryId,
  onDone,
  onClose,
}: {
  ids: number[];
  /** 옮겨 넣을 라이브러리 */
  libraryId: number;
  onDone: (o: Outcome) => void;
  onClose: () => void;
}) {
  const [date, setDate] = useState("");
  const [title, setTitle] = useState("");
  const [tips, setTips] = useState<Suggestion[]>([]);
  const [preview, setPreview] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<string>("organize_date", { ids })
      .then(setDate)
      .catch(() => {});
    invoke<Suggestion[]>("organize_suggest", { ids })
      .then(setTips)
      .catch(() => setTips([]));
  }, [ids]);

  useEffect(() => {
    if (!date) return;
    invoke<string>("organize_preview", { date, title })
      .then(setPreview)
      .catch(() => setPreview(""));
  }, [date, title]);

  const run = useCallback(async () => {
    if (!date) return;
    setBusy(true);
    setError(null);
    try {
      const o = await invoke<Outcome>("organize_move", {
        ids,
        libraryId,
        date,
        title,
      });
      if (o.failed > 0) setError(`${o.failed}장 실패 — ${o.first_error ?? ""}`);
      onDone(o);
      if (o.failed === 0) onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [ids, libraryId, date, title, onDone, onClose]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) run();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, run]);

  return (
    <div className="absolute inset-0 z-40 bg-canvas/95 backdrop-blur-sm flex items-center justify-center p-6">
      <div className="w-[520px] max-w-full bg-chrome rounded-xl ring-1 ring-line shadow-2xl p-5">
        <div className="flex items-baseline gap-2 mb-4">
          <span className="text-[15px] font-semibold text-fg">정리</span>
          <span className="text-[12px] text-fg-mute">
            {ids.length.toLocaleString()}장을 이벤트 폴더로 옮깁니다
          </span>
        </div>

        <div className="flex gap-2 mb-3">
          <label className="flex flex-col gap-1">
            <span className="text-[10.5px] uppercase tracking-wider text-fg-mute">
              날짜
            </span>
            <input
              type="date"
              value={date}
              onChange={(e) => setDate(e.target.value)}
              className="h-8 px-2 rounded bg-raised text-[13px] text-fg ring-1 ring-line outline-none focus:ring-accent"
            />
          </label>
          <label className="flex-1 flex flex-col gap-1 min-w-0">
            <span className="text-[10.5px] uppercase tracking-wider text-fg-mute">
              이벤트 이름
            </span>
            <input
              value={title}
              autoFocus
              onChange={(e) => setTitle(e.target.value)}
              placeholder="예: 거제통영 가족여행"
              className="h-8 px-2 rounded bg-raised text-[13px] text-fg placeholder:text-fg-faint ring-1 ring-line outline-none focus:ring-accent"
            />
          </label>
        </div>

        {tips.length > 0 && (
          <div className="mb-3">
            <div className="text-[10.5px] uppercase tracking-wider text-fg-mute mb-1.5">
              제안
            </div>
            <div className="flex flex-wrap gap-1.5">
              {tips.map((t) => (
                <button
                  key={t.title}
                  onClick={() => setTitle(t.title)}
                  title={t.why}
                  className={`h-6 px-2 rounded text-[12px] ${
                    title === t.title
                      ? "bg-accent text-accent-fg"
                      : "text-fg-dim ring-1 ring-line hover:text-white"
                  }`}
                >
                  {t.title}
                </button>
              ))}
            </div>
            {/* 왜 권하는지 — 고르기 전에 근거가 보여야 믿고 누른다 */}
            {tips.find((t) => t.title === title)?.why && (
              <div className="text-[11px] text-fg-mute mt-1.5">
                {tips.find((t) => t.title === title)?.why}
              </div>
            )}
          </div>
        )}

        <div className="text-[10.5px] uppercase tracking-wider text-fg-mute mb-1">
          옮겨질 곳
        </div>
        <div className="px-2.5 py-2 rounded bg-raised font-mono text-[12px] text-accent break-all mb-4">
          {preview || "날짜를 정하세요"}
        </div>

        {error && <div className="text-[12px] text-drop mb-3">{error}</div>}

        <div className="flex items-center gap-2">
          <button
            onClick={run}
            disabled={!date || busy}
            className="h-8 px-4 rounded-lg bg-accent text-accent-fg font-semibold text-[13px] disabled:opacity-40"
          >
            {busy ? "옮기는 중…" : "옮기기"}
          </button>
          <button
            onClick={onClose}
            className="h-8 px-3 rounded-lg text-fg-dim text-[13px] ring-1 ring-line-strong"
          >
            취소
          </button>
          <div className="flex-1" />
          <span className="text-[11px] text-fg-mute">되돌릴 수 있습니다</span>
        </div>
      </div>
    </div>
  );
}
