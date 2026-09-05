import { useCallback, useEffect, useId, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useData } from "./dataStore";
import { AREAS, nextArea } from "./areaItems";
import type { Outcome } from "./types";
import { useModalFocus } from "./focus";

type Suggestion = { title: string; why: string; score: number };
export type OrganizeOutcome = Outcome & {
  copied: number;
  already_published: number;
  mode: "move" | "publish_copy";
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
  onDone: (o: OrganizeOutcome) => void | Promise<void>;
  onClose: () => void;
}) {
  const libs = useData((s) => s.libs);
  // 부분 실패 뒤 목록을 다시 읽으면 깊은 페이지의 실패 사진은 전역 선택에서
  // 빠질 수 있다. 이 창은 자기 작업 대상을 따로 들고 있어 실패한 것만 확실히
  // 다시 시도한다.
  const [workIds, setWorkIds] = useState(ids);
  // 목적지 — 흐름의 다음 칸이 기본. 작업대에서 정리하면 내사진, 내사진에서면 공용.
  // 그 영역에 라이브러리가 없으면 지금 라이브러리에 그대로.
  const [dest, setDest] = useState<number>(() => {
    const cur = libs.find((l) => l.id === libraryId);
    const want = cur ? nextArea(cur.area) : null;
    const next =
      want === null ? null : libs.find((l) => l.area === want && l.online);
    return next?.id ?? libraryId;
  });
  const [date, setDate] = useState("");
  const [title, setTitle] = useState("");
  const [tips, setTips] = useState<Suggestion[]>([]);
  const [preview, setPreview] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sourceArea = libs.find((library) => library.id === libraryId)?.area;
  const destinationArea = libs.find((library) => library.id === dest)?.area;
  const publishing = sourceArea === 1 && destinationArea === 2;
  const titleId = useId();
  const initialIds = useRef(ids).current;

  useEffect(() => {
    invoke<string>("organize_date", { ids: initialIds })
      .then(setDate)
      .catch(() => {});
  }, [initialIds]);

  useEffect(() => {
    invoke<Suggestion[]>("organize_suggest", { ids: workIds })
      .then(setTips)
      .catch(() => setTips([]));
  }, [workIds]);

  useEffect(() => {
    if (!date) return;
    invoke<string>("organize_preview", { libraryId: dest, date, title })
      .then(setPreview)
      .catch(() => setPreview(""));
  }, [dest, date, title]);

  const run = useCallback(async () => {
    if (!date) return;
    setBusy(true);
    setError(null);
    try {
      const o = await invoke<OrganizeOutcome>("organize_move", {
        ids: workIds,
        libraryId: dest,
        date,
        title,
      });
      if (o.failed > 0) {
        setError(`${o.failed}장 실패 — ${o.first_error ?? ""}`);
        setWorkIds(o.failed_ids?.length ? o.failed_ids : workIds);
      }
      await onDone(o);
      if (o.failed === 0) onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [workIds, dest, date, title, onDone, onClose]);

  const dialogRef = useRef<HTMLDivElement>(null);
  useModalFocus(dialogRef, onClose, { locked: busy });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) run();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [run]);

  return (
    <div className="absolute inset-0 z-40 bg-canvas/95 backdrop-blur-sm flex items-center justify-center p-6">
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="w-[520px] max-w-full bg-chrome rounded-xl ring-1 ring-line shadow-2xl p-5"
      >
        <div className="flex items-baseline gap-2 mb-4">
          <span id={titleId} className="text-[16px] font-semibold text-fg">
            정리
          </span>
          <span className="text-[13px] text-fg-mute">
            {workIds.length.toLocaleString()}장을 이벤트 폴더로{" "}
            {publishing ? "복사합니다" : "옮깁니다"}
          </span>
        </div>

        {/* 어디로 — 영역별 라이브러리. 물리적 위치가 곧 처리 단계라 이게 먼저다 */}
        <div className="mb-3">
          <div className="text-[11.5px] uppercase tracking-wider text-fg-mute mb-1.5">
            어디로
          </div>
          <div className="flex flex-wrap gap-1.5">
            {AREAS.map((a) =>
              libs
                .filter((l) => l.area === a.v)
                .map((l) => (
                  <button
                    key={l.id}
                    onClick={() => l.online && setDest(l.id)}
                    disabled={!l.online}
                    title={
                      l.online ? a.hint : "디스크가 연결되어 있지 않습니다"
                    }
                    className={`h-6 px-2 rounded text-[13px] disabled:opacity-40 ${
                      dest === l.id
                        ? "bg-accent text-accent-fg"
                        : "text-fg-dim ring-1 ring-line hover:text-white"
                    }`}
                  >
                    <span className="text-fg-mute mr-1">{a.label}</span>
                    {l.name}
                  </button>
                )),
            )}
          </div>
        </div>

        <div className="flex gap-2 mb-3">
          <label className="flex flex-col gap-1">
            <span className="text-[11.5px] uppercase tracking-wider text-fg-mute">
              날짜
            </span>
            <input
              type="date"
              value={date}
              onChange={(e) => setDate(e.target.value)}
              className="h-control px-2 rounded bg-raised text-[14px] text-fg ring-1 ring-line outline-none focus:ring-accent"
            />
          </label>
          <label className="flex-1 flex flex-col gap-1 min-w-0">
            <span className="text-[11.5px] uppercase tracking-wider text-fg-mute">
              이벤트 이름
            </span>
            <input
              value={title}
              autoFocus
              onChange={(e) => setTitle(e.target.value)}
              placeholder="예: 거제통영 가족여행"
              className="h-control px-2 rounded bg-raised text-[14px] text-fg placeholder:text-fg-faint ring-1 ring-line outline-none focus:ring-accent"
            />
          </label>
        </div>

        {tips.length > 0 && (
          <div className="mb-3">
            <div className="text-[11.5px] uppercase tracking-wider text-fg-mute mb-1.5">
              제안
            </div>
            <div className="flex flex-wrap gap-1.5">
              {tips.map((t) => (
                <button
                  key={t.title}
                  onClick={() => setTitle(t.title)}
                  title={t.why}
                  className={`h-6 px-2 rounded text-[13px] ${
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
              <div className="text-[12px] text-fg-mute mt-1.5">
                {tips.find((t) => t.title === title)?.why}
              </div>
            )}
          </div>
        )}

        <div className="text-[11.5px] uppercase tracking-wider text-fg-mute mb-1">
          {publishing ? "복사될 곳" : "옮겨질 곳"}
        </div>
        <div className="px-2.5 py-2 rounded bg-raised font-mono text-[13px] text-accent break-all mb-4">
          {preview || "날짜를 정하세요"}
        </div>

        {error && (
          <div role="alert" className="text-[13px] text-drop mb-3">
            {error}
          </div>
        )}

        {publishing && (
          <div className="mb-3 rounded bg-raised px-2.5 py-2 text-[12.5px] text-fg-dim ring-1 ring-line">
            내사진 원본은 그대로 두고 공용에 사본을 발행합니다. 같은 사진을 다시
            실행하면 해시 원장이 중복 사본을 막습니다.
          </div>
        )}

        <div className="flex items-center gap-2">
          <button
            onClick={run}
            disabled={!date || busy}
            className="h-control px-3.5 rounded-lg bg-accent text-accent-fg font-semibold text-[14px] disabled:opacity-40"
          >
            {busy
              ? publishing
                ? "복사하는 중…"
                : "옮기는 중…"
              : publishing
                ? "공용에 복사"
                : "옮기기"}
          </button>
          <button
            onClick={onClose}
            className="h-control px-3 rounded-lg text-fg-dim text-[14px] ring-1 ring-line-strong"
          >
            취소
          </button>
          <div className="flex-1" />
          <span className="text-[12px] text-fg-mute">되돌릴 수 있습니다</span>
        </div>
      </div>
    </div>
  );
}
