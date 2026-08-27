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
    <div className="absolute inset-0 z-40 bg-[#15191A]/95 backdrop-blur-sm flex items-center justify-center p-6">
      <div className="w-[520px] max-w-full bg-[#1C2123] rounded-xl ring-1 ring-[#2E383A] shadow-2xl p-5">
        <div className="flex items-baseline gap-2 mb-4">
          <span className="text-[15px] font-semibold text-[#EAEFEF]">정리</span>
          <span className="text-[12px] text-[#7C8A8D]">
            {ids.length.toLocaleString()}장을 이벤트 폴더로 옮깁니다
          </span>
        </div>

        <div className="flex gap-2 mb-3">
          <label className="flex flex-col gap-1">
            <span className="text-[10.5px] uppercase tracking-wider text-[#5F6C6E]">
              날짜
            </span>
            <input
              type="date"
              value={date}
              onChange={(e) => setDate(e.target.value)}
              className="h-8 px-2 rounded bg-[#141A1B] text-[13px] text-[#EAEFEF] ring-1 ring-[#2A3335] outline-none focus:ring-[#49B8B4]"
            />
          </label>
          <label className="flex-1 flex flex-col gap-1 min-w-0">
            <span className="text-[10.5px] uppercase tracking-wider text-[#5F6C6E]">
              이벤트 이름
            </span>
            <input
              value={title}
              autoFocus
              onChange={(e) => setTitle(e.target.value)}
              placeholder="예: 거제통영 가족여행"
              className="h-8 px-2 rounded bg-[#141A1B] text-[13px] text-[#EAEFEF] placeholder:text-[#4E5A5C] ring-1 ring-[#2A3335] outline-none focus:ring-[#49B8B4]"
            />
          </label>
        </div>

        {tips.length > 0 && (
          <div className="mb-3">
            <div className="text-[10.5px] uppercase tracking-wider text-[#5F6C6E] mb-1.5">
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
                      ? "bg-[#49B8B4] text-[#08191a]"
                      : "text-[#A3B2B4] ring-1 ring-[#2E383A] hover:text-white"
                  }`}
                >
                  {t.title}
                </button>
              ))}
            </div>
            {/* 왜 권하는지 — 고르기 전에 근거가 보여야 믿고 누른다 */}
            {tips.find((t) => t.title === title)?.why && (
              <div className="text-[11px] text-[#6D7B7E] mt-1.5">
                {tips.find((t) => t.title === title)?.why}
              </div>
            )}
          </div>
        )}

        <div className="text-[10.5px] uppercase tracking-wider text-[#5F6C6E] mb-1">
          옮겨질 곳
        </div>
        <div className="px-2.5 py-2 rounded bg-[#141A1B] font-mono text-[12px] text-[#49B8B4] break-all mb-4">
          {preview || "날짜를 정하세요"}
        </div>

        {error && (
          <div className="text-[12px] text-[#E2685C] mb-3">{error}</div>
        )}

        <div className="flex items-center gap-2">
          <button
            onClick={run}
            disabled={!date || busy}
            className="h-8 px-4 rounded-lg bg-[#49B8B4] text-[#08191a] font-semibold text-[13px] disabled:opacity-40"
          >
            {busy ? "옮기는 중…" : "옮기기"}
          </button>
          <button
            onClick={onClose}
            className="h-8 px-3 rounded-lg text-[#A3B2B4] text-[13px] ring-1 ring-[#333C3F]"
          >
            취소
          </button>
          <div className="flex-1" />
          <span className="text-[11px] text-[#5F6C6E]">되돌릴 수 있습니다</span>
        </div>
      </div>
    </div>
  );
}
