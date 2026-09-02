import { useCallback, useEffect, useId, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type AuditItem = {
  id: number;
  name: string;
  path: string;
  current_at: number;
  current_source: number;
  proposed_at: number | null;
  proposed_source: string | null;
  evidence: string;
  interpretation: string;
  write_scope: string;
  auto_selected: boolean;
  existing_exif: boolean;
};

type CaptureOutcome = {
  batch_id: number;
  corrected: number;
  failed: number;
  first_error: string | null;
  failed_ids: number[];
};

const localInput = (d = new Date()) => {
  const shifted = new Date(d.getTime() - d.getTimezoneOffset() * 60_000);
  return shifted.toISOString().slice(0, 16);
};

export default function CaptureDateDialog({
  target,
  onChanged,
  onClose,
}: {
  target: { ids: number[]; libraryId?: number; relPath?: string };
  onChanged: () => void | Promise<void>;
  onClose: () => void;
}) {
  const titleId = useId();
  const [rows, setRows] = useState<AuditItem[]>([]);
  const [checked, setChecked] = useState<Set<number>>(new Set());
  const [manualAt, setManualAt] = useState(localInput());
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const found = await invoke<AuditItem[]>("capture_date_audit", {
        target: {
          ids: target.ids,
          libraryId: target.libraryId ?? null,
          relPath: target.relPath ?? null,
          recursive: true,
        },
      });
      setRows(found);
      setChecked(new Set(found.filter((r) => r.auto_selected).map((r) => r.id)));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [target]);

  useEffect(() => void load(), [load]);
  useEffect(() => {
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onClose]);

  const chosen = useMemo(() => rows.filter((r) => checked.has(r.id)), [rows, checked]);
  const run = async (manual: boolean) => {
    const manualTs = Math.floor(new Date(manualAt).getTime() / 1000);
    const changes = chosen
      .map((r) => ({ id: r.id, takenAt: manual ? manualTs : r.proposed_at, manual }))
      .filter((r): r is { id: number; takenAt: number; manual: boolean } => Number.isFinite(r.takenAt));
    if (changes.length === 0) return;
    setBusy(true);
    setError(null);
    try {
      const out = await invoke<CaptureOutcome>("capture_date_apply", {
        changes,
        label: manual ? "촬영일 수동 교정" : "촬영일 자동 교정",
      });
      if (out.failed > 0) {
        setError(`${out.corrected}장 교정 · ${out.failed}장 실패 — ${out.first_error ?? ""}`);
        setChecked(new Set(out.failed_ids));
      } else {
        await onChanged();
        await load();
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-[65] bg-canvas/95 backdrop-blur-sm flex items-center justify-center p-6">
      <div role="dialog" aria-modal="true" aria-labelledby={titleId} className="w-[920px] max-w-full max-h-[86vh] flex flex-col bg-chrome rounded-xl ring-1 ring-line shadow-2xl p-5">
        <div className="flex items-baseline gap-2 mb-3">
          <h2 id={titleId} className="text-[16px] font-semibold text-fg">촬영일 감사·교정</h2>
          <span className="text-[13px] text-fg-mute">먼저 읽기만 한 dry-run 결과입니다</span>
        </div>
        <div className="grid grid-cols-[28px_minmax(140px,1fr)_150px_150px_minmax(230px,1.4fr)] gap-2 px-2 pb-1 text-[11px] uppercase tracking-wider text-fg-mute">
          <span /> <span>파일</span><span>현재</span><span>제안</span><span>근거 · 기록 범위</span>
        </div>
        <div className="flex-1 min-h-0 overflow-auto rounded ring-1 ring-line bg-canvas">
          {busy && rows.length === 0 ? <div className="p-5 text-fg-mute">감사하는 중…</div> : rows.length === 0 ? <div className="p-5 text-fg-mute">감사할 사진이 없습니다.</div> : rows.map((r) => (
            <label key={r.id} className="grid grid-cols-[28px_minmax(140px,1fr)_150px_150px_minmax(230px,1.4fr)] gap-2 items-start px-2 py-2 border-b border-line last:border-0 text-[12px]">
              <input type="checkbox" checked={checked.has(r.id)} onChange={(e) => setChecked((old) => { const next = new Set(old); if (e.target.checked) next.add(r.id); else next.delete(r.id); return next; })} />
              <span className="min-w-0"><span className="block truncate text-fg" title={r.path}>{r.name}</span><span className="block truncate text-fg-mute" title={r.path}>{r.path}</span></span>
              <span className="text-fg-dim tabular-nums">{new Date(r.current_at * 1000).toLocaleString()}<small className="block text-fg-mute">출처 {r.current_source}</small></span>
              <span className="text-accent tabular-nums">{r.proposed_at ? new Date(r.proposed_at * 1000).toLocaleString() : "수동 지정 필요"}<small className="block text-fg-mute">{r.proposed_source ?? "근거 없음"}</small></span>
              <span className="text-fg-dim">{r.evidence}<small className="block text-fg-mute">{r.interpretation}</small><small className="block text-fg-mute">{r.write_scope}</small>{r.existing_exif && <small className="block text-keep">유효한 파일 내부 촬영일 — 자동 덮어쓰기 안 함</small>}</span>
            </label>
          ))}
        </div>
        {error && <div role="alert" className="mt-3 text-[13px] text-drop">{error}</div>}
        <div className="mt-4 flex flex-wrap items-end gap-2">
          <button onClick={() => run(false)} disabled={busy || chosen.every((r) => !r.auto_selected)} className="h-control px-3 rounded-lg bg-accent text-accent-fg font-semibold disabled:opacity-40">선택한 자동 후보 교정</button>
          <label className="flex flex-col gap-1"><span className="text-[11px] uppercase tracking-wider text-fg-mute">선택 전체에 같은 지역 날짜·시각</span><input type="datetime-local" value={manualAt} onChange={(e) => setManualAt(e.target.value)} className="h-control px-2 rounded bg-raised ring-1 ring-line text-fg" /></label>
          <button onClick={() => run(true)} disabled={busy || chosen.length === 0 || !Number.isFinite(new Date(manualAt).getTime())} className="h-control px-3 rounded-lg text-fg ring-1 ring-line-strong disabled:opacity-40">수동 일괄 교정</button>
          <div className="flex-1" />
          <button onClick={onClose} className="h-control px-3 rounded-lg text-fg-dim ring-1 ring-line-strong">닫기</button>
        </div>
        <p className="mt-2 text-[11.5px] text-fg-mute">JPEG는 EXIF DateTimeOriginal·DateTimeDigitized·TIFF DateTime과 mtime을 기록합니다. HEIC·RAW·PNG·영상은 파일 내부 메타데이터를 바꾸지 않고 mtime과 Photo Desk 보정값만 기록합니다. 모든 성공 항목은 배치 단위로 되돌릴 수 있습니다.</p>
      </div>
    </div>
  );
}
