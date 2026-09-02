import { useEffect, useId, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useData } from "./dataStore";
import { areaLabel, nextArea } from "./areaItems";

type Mode = "move" | "copy";
type Policy = "skip" | "rename";
type Preview = {
  mode: Mode;
  publish: boolean;
  source_area: number | null;
  destination_area: number;
  drive_sync_warning: boolean;
  items: {
    id: number;
    source: string;
    destination: string;
    planned_name: string;
    conflict: string;
    action: string;
    source_sha256: string | null;
  }[];
};
type Result = {
  batch_id: number;
  completed: number;
  failed: number;
  skipped: number;
  already_published: number;
  first_error: string | null;
  failed_ids: number[];
};

export default function TransferDialog({ ids, sourceLibraryId, onChanged, onClose }: {
  ids: number[];
  sourceLibraryId: number;
  onChanged: () => void | Promise<void>;
  onClose: () => void;
}) {
  const titleId = useId();
  const libs = useData((s) => s.libs);
  const folders = useData((s) => s.folders);
  const source = libs.find((l) => l.id === sourceLibraryId);
  const initialDest = useMemo(() => {
    const area = nextArea(source?.area ?? 3);
    return libs.find((l) => l.online && l.area === area)?.id ?? sourceLibraryId;
  }, [libs, source?.area, sourceLibraryId]);
  const [destinationLibraryId, setDestinationLibraryId] = useState(initialDest);
  const [destinationDir, setDestinationDir] = useState("");
  const [newFolder, setNewFolder] = useState(false);
  const [mode, setMode] = useState<Mode>(() => source?.area === 1 && libs.find((l) => l.id === initialDest)?.area === 2 ? "copy" : "move");
  const [policy, setPolicy] = useState<Policy>("skip");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const destination = libs.find((l) => l.id === destinationLibraryId);
  const publish = source?.area === 1 && destination?.area === 2 && mode === "copy";
  const choices = folders.filter((f) => f.library_id === destinationLibraryId && !f.is_library);

  useEffect(() => {
    setMode(source?.area === 1 && destination?.area === 2 ? "copy" : "move");
  }, [source?.area, destination?.area]);
  useEffect(() => {
    setDestinationDir("");
    setPreview(null);
  }, [destinationLibraryId, newFolder, mode, policy]);
  useEffect(() => {
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onClose]);

  const request = { ids, destinationLibraryId, destinationDir, mode, conflictPolicy: policy, publish };
  const inspect = async () => {
    setBusy(true); setError(null);
    try { setPreview(await invoke<Preview>("transfer_preview", { request })); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  const execute = async () => {
    if (!preview) return;
    setBusy(true); setError(null);
    try {
      const out = await invoke<Result>("transfer_execute", {
        request,
        label: publish ? "내사진 → 공용 발행" : `사진 ${mode === "copy" ? "복사" : "이동"}`,
      });
      if (out.completed > 0) await onChanged();
      if (out.failed > 0) {
        setError(`${out.completed}장 완료 · ${out.failed}장 실패 — ${out.first_error ?? ""}`);
      } else if (out.completed === 0) {
        const reason = out.already_published > 0
          ? `${out.already_published}장이 이미 발행되어 건너뛰었습니다.`
          : `${out.skipped}장을 충돌 때문에 건너뛰었습니다.`;
        setError(`${reason} 미리보기와 충돌 정책을 확인하세요.`);
      } else {
        onClose();
      }
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  const conflicts = preview?.items.filter((i) => i.conflict !== "none").length ?? 0;
  return (
    <div className="fixed inset-0 z-[66] bg-canvas/95 backdrop-blur-sm flex items-center justify-center p-6">
      <div role="dialog" aria-modal="true" aria-labelledby={titleId} className="w-[720px] max-w-full max-h-[86vh] flex flex-col bg-chrome rounded-xl ring-1 ring-line shadow-2xl p-5">
        <div className="flex items-baseline gap-2 mb-4"><h2 id={titleId} className="text-[16px] font-semibold text-fg">사진 이동·복사</h2><span className="text-[13px] text-fg-mute">{ids.length.toLocaleString()}장</span></div>
        <div className="grid grid-cols-2 gap-3 mb-3">
          <label className="flex flex-col gap-1"><span className="text-[11px] uppercase tracking-wider text-fg-mute">목적지 라이브러리</span><select value={destinationLibraryId} onChange={(e) => setDestinationLibraryId(Number(e.target.value))} className="h-control px-2 rounded bg-raised ring-1 ring-line text-fg">{libs.map((l) => <option key={l.id} value={l.id} disabled={!l.online}>{areaLabel(l.area)} · {l.name}{!l.online ? " (연결 안 됨)" : ""}</option>)}</select></label>
          <div className="flex flex-col gap-1"><span className="text-[11px] uppercase tracking-wider text-fg-mute">동작</span><div className="flex gap-1"><button onClick={() => setMode("move")} className={`h-control px-3 rounded ring-1 ${mode === "move" ? "bg-accent text-accent-fg ring-accent" : "text-fg-dim ring-line"}`}>이동</button><button onClick={() => setMode("copy")} className={`h-control px-3 rounded ring-1 ${mode === "copy" ? "bg-accent text-accent-fg ring-accent" : "text-fg-dim ring-line"}`}>복사</button></div></div>
        </div>
        <div className="flex gap-2 mb-3"><button onClick={() => setNewFolder(false)} className={`h-control px-3 rounded ring-1 ${!newFolder ? "text-accent ring-accent" : "text-fg-dim ring-line"}`}>기존 폴더</button><button onClick={() => setNewFolder(true)} className={`h-control px-3 rounded ring-1 ${newFolder ? "text-accent ring-accent" : "text-fg-dim ring-line"}`}>새 폴더</button>{newFolder ? <input value={destinationDir} onChange={(e) => { setDestinationDir(e.target.value); setPreview(null); }} placeholder="예: 2026/가족여행" className="h-control flex-1 px-2 rounded bg-raised ring-1 ring-line text-fg" /> : <select value={destinationDir} onChange={(e) => { setDestinationDir(e.target.value); setPreview(null); }} className="h-control flex-1 px-2 rounded bg-raised ring-1 ring-line text-fg"><option value="">라이브러리 바로 아래</option>{choices.map((f) => <option key={f.path} value={f.path.split("/").slice(1).join("/")}>{f.path.split("/").slice(1).join("/")}</option>)}</select>}</div>
        {publish && <div className="mb-3 px-3 py-2 rounded text-[13px] text-keep ring-1 ring-keep/50 bg-keep/10">내사진 → 공용은 복사가 기본입니다. 개인 원본을 유지하고 SHA-256 발행 원장으로 재실행 중복을 막습니다.</div>}
        {preview?.drive_sync_warning && <div className="mb-3 px-3 py-2 rounded text-[12.5px] text-drop ring-1 ring-drop/50 bg-drop/10">내사진/공용은 Drive 동기화 폴더입니다. 이동·이름 변경·삭제는 NAS에도 반영될 수 있습니다.</div>}
        <div className="flex items-end gap-2 mb-3"><label className="flex flex-col gap-1"><span className="text-[11px] uppercase tracking-wider text-fg-mute">같은 이름 충돌</span><select value={policy} onChange={(e) => setPolicy(e.target.value as Policy)} className="h-control px-2 rounded bg-raised ring-1 ring-line text-fg"><option value="skip">건너뜀</option><option value="rename">새 이름</option></select></label><button onClick={inspect} disabled={busy || !destination?.online} className="h-control px-3 rounded bg-accent text-accent-fg font-semibold disabled:opacity-40">충돌 미리보기</button>{preview && <span className="pb-1 text-[12px] text-fg-mute">충돌/기존 발행 {conflicts}건</span>}</div>
        {preview && <div className="flex-1 min-h-0 overflow-auto rounded ring-1 ring-line bg-canvas mb-3">{preview.items.map((i) => <div key={i.id} className="grid grid-cols-[1fr_1fr_110px] gap-2 px-3 py-2 border-b border-line last:border-0 text-[12px]"><span className="truncate text-fg-dim" title={i.source}>{i.source}</span><span className="truncate text-fg" title={i.destination}>→ {i.destination}</span><span className={i.action === "skip" ? "text-fg-mute" : i.conflict === "none" ? "text-keep" : "text-accent"}>{i.conflict === "already_published" ? "이미 발행 · 건너뜀" : i.conflict === "source_missing" ? "원본 없음" : i.action === "rename" ? "새 이름" : i.action === "skip" ? "충돌 · 건너뜀" : "실행"}</span></div>)}</div>}
        {error && <div role="alert" className="mb-3 text-[13px] text-drop">{error}</div>}
        <div className="flex justify-end gap-2"><button onClick={onClose} className="h-control px-3 rounded ring-1 ring-line text-fg-dim">취소</button><button onClick={execute} disabled={busy || !preview} className="h-control px-3 rounded bg-accent text-accent-fg font-semibold disabled:opacity-40">{busy ? "작업 중…" : `${mode === "copy" ? "복사" : "이동"} 실행`}</button></div>
      </div>
    </div>
  );
}
