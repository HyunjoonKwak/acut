import { useEffect, useId, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useData } from "./dataStore";
import { areaLabel } from "./areaItems";

export type FolderAction = "create" | "rename" | "move" | "copy" | "trash";

export type FolderOperationTarget = {
  action: FolderAction;
  sourceLibraryId: number;
  sourceDir: string;
  sourceName: string;
};

type Policy = "skip" | "rename";
type Preview = {
  source: string;
  destination: string;
  planned_name: string;
  conflict: string;
  action: string;
  files: number;
  directories: number;
  bytes: number;
  cross_volume: boolean;
  drive_sync_warning: boolean;
};
type Result = {
  batch_id: number;
  completed: number;
  failed: number;
  files: number;
  directories: number;
  bytes: number;
  first_error: string | null;
  manifest_sha256: string | null;
};

const labels: Record<FolderAction, string> = {
  create: "새 폴더",
  rename: "이름 변경",
  move: "폴더 이동",
  copy: "폴더 복사",
  trash: "폴더를 휴지통으로",
};

const humanBytes = (bytes: number) =>
  bytes < 1024
    ? `${bytes} B`
    : bytes < 1024 ** 2
      ? `${(bytes / 1024).toFixed(1)} KB`
      : `${(bytes / 1024 ** 2).toFixed(1)} MB`;

export default function FolderOperationDialog({ target, onChanged, onClose }: {
  target: FolderOperationTarget;
  onChanged: () => void | Promise<void>;
  onClose: () => void;
}) {
  const titleId = useId();
  const libs = useData((s) => s.libs);
  const folders = useData((s) => s.folders);
  const [action, setAction] = useState<FolderAction>(target.action);
  const [destinationLibraryId, setDestinationLibraryId] = useState(target.sourceLibraryId);
  const sourceParent = target.sourceDir.split("/").slice(0, -1).join("/");
  const [destinationParent, setDestinationParent] = useState(sourceParent);
  const [name, setName] = useState(target.action === "create" ? "" : target.sourceName);
  const [policy, setPolicy] = useState<Policy>("skip");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const destination = libs.find((library) => library.id === destinationLibraryId);
  const choices = useMemo(
    () =>
      folders.filter(
        (folder) =>
          folder.library_id === destinationLibraryId && !folder.is_library,
      ),
    [destinationLibraryId, folders],
  );

  useEffect(() => {
    const key = (event: KeyboardEvent) => event.key === "Escape" && onClose();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onClose]);

  useEffect(() => {
    setPreview(null);
    setError(null);
  }, [action, destinationLibraryId, destinationParent, name, policy]);

  const request = {
    action,
    sourceLibraryId: target.sourceLibraryId,
    sourceDir: target.sourceDir,
    destinationLibraryId:
      action === "move" || action === "copy" || action === "rename"
        ? destinationLibraryId
        : null,
    destinationParent:
      action === "move" || action === "copy"
        ? destinationParent
        : action === "rename"
          ? sourceParent
          : null,
    name: action === "trash" ? null : name,
    conflictPolicy: policy,
  };

  const inspect = async () => {
    setBusy(true);
    setError(null);
    try {
      setPreview(
        await invoke<Preview>("folder_operation_preview", { request }),
      );
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
    }
  };

  const execute = async () => {
    if (!preview || preview.action === "skip") return;
    setBusy(true);
    setError(null);
    try {
      const result = await invoke<Result>("folder_operation_execute", {
        request,
        label: `${target.sourceName || "라이브러리"} — ${labels[action]}`,
      });
      if (result.failed > 0) {
        setError(result.first_error ?? "폴더 작업을 완료하지 못했습니다");
      } else {
        await onChanged();
        onClose();
      }
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
    }
  };

  const needsDestination = action === "move" || action === "copy";
  const needsName = action === "create" || action === "rename" || needsDestination;
  return (
    <div className="fixed inset-0 z-[67] bg-canvas/95 backdrop-blur-sm flex items-center justify-center p-6">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="w-[660px] max-w-full max-h-[86vh] overflow-auto bg-chrome rounded-xl ring-1 ring-line shadow-2xl p-5"
      >
        <h2 id={titleId} className="text-[16px] font-semibold text-fg mb-1">
          폴더 작업
        </h2>
        <p className="text-[12.5px] text-fg-mute mb-4 truncate" title={target.sourceDir}>
          {target.sourceDir || "라이브러리 바로 아래"}
        </p>

        <div className="flex flex-wrap gap-1 mb-4" aria-label="폴더 작업 종류">
          {(target.sourceDir
            ? (["create", "rename", "move", "copy", "trash"] as FolderAction[])
            : (["create"] as FolderAction[])
          ).map((value) => (
            <button
              key={value}
              onClick={() => setAction(value)}
              className={`h-control px-3 rounded ring-1 ${
                action === value
                  ? value === "trash"
                    ? "text-drop ring-drop bg-drop/10"
                    : "text-accent ring-accent bg-accent/10"
                  : "text-fg-dim ring-line"
              }`}
            >
              {labels[value]}
            </button>
          ))}
        </div>

        {needsDestination && (
          <div className="grid grid-cols-2 gap-3 mb-3">
            <label className="flex flex-col gap-1">
              <span className="text-[11px] uppercase tracking-wider text-fg-mute">목적지 라이브러리</span>
              <select
                value={destinationLibraryId}
                onChange={(event) => {
                  setDestinationLibraryId(Number(event.target.value));
                  setDestinationParent("");
                }}
                className="h-control px-2 rounded bg-raised ring-1 ring-line text-fg"
              >
                {libs.map((library) => (
                  <option key={library.id} value={library.id} disabled={!library.online}>
                    {areaLabel(library.area)} · {library.name}
                    {!library.online ? " (연결 안 됨)" : ""}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-[11px] uppercase tracking-wider text-fg-mute">목적지 부모</span>
              <select
                value={destinationParent}
                onChange={(event) => setDestinationParent(event.target.value)}
                className="h-control px-2 rounded bg-raised ring-1 ring-line text-fg"
              >
                <option value="">라이브러리 바로 아래</option>
                {choices.map((folder) => {
                  const path = folder.path.split("/").slice(1).join("/");
                  return (
                    <option key={folder.path} value={path}>
                      {path}
                    </option>
                  );
                })}
              </select>
            </label>
          </div>
        )}

        {needsName && (
          <label className="flex flex-col gap-1 mb-3">
            <span className="text-[11px] uppercase tracking-wider text-fg-mute">폴더 이름</span>
            <input
              autoFocus
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="새 폴더 이름"
              className="h-control px-2 rounded bg-raised ring-1 ring-line text-fg"
            />
          </label>
        )}

        {(action === "move" || action === "copy" || action === "rename" || action === "create") && (
          <label className="flex items-center gap-2 mb-3 text-[12.5px] text-fg-dim">
            <span>같은 이름 충돌</span>
            <select
              value={policy}
              onChange={(event) => setPolicy(event.target.value as Policy)}
              className="h-control px-2 rounded bg-raised ring-1 ring-line text-fg"
            >
              <option value="skip">실행 안 함</option>
              <option value="rename">번호를 붙여 새 이름</option>
            </select>
          </label>
        )}

        {action === "trash" && (
          <div className="mb-3 px-3 py-2 rounded text-[12.5px] text-drop ring-1 ring-drop/50 bg-drop/10">
            폴더 전체를 Photo Desk 휴지통으로 옮깁니다. 라이브러리 루트와 원본 밖 경로는 차단됩니다.
          </div>
        )}
        {preview?.drive_sync_warning && (
          <div className="mb-3 px-3 py-2 rounded text-[12.5px] text-drop ring-1 ring-drop/50 bg-drop/10">
            내사진/공용은 Drive 동기화 폴더입니다. 이동·이름 변경·휴지통은 동기화 대상에도 반영될 수 있습니다.
          </div>
        )}

        <div className="flex items-center gap-2 mb-3">
          <button
            onClick={inspect}
            disabled={busy || !destination?.online || (needsName && !name.trim())}
            className="h-control px-3 rounded bg-accent text-accent-fg font-semibold disabled:opacity-40"
          >
            충돌 미리보기
          </button>
          {preview && (
            <span className="text-[12px] text-fg-mute">
              {preview.directories.toLocaleString()}폴더 · {preview.files.toLocaleString()}파일 · {humanBytes(preview.bytes)}
              {preview.cross_volume ? " · 다른 볼륨" : ""}
            </span>
          )}
        </div>

        {preview && (
          <div className="mb-3 rounded ring-1 ring-line bg-canvas px-3 py-2 text-[12.5px]">
            <div className="text-fg-dim truncate" title={preview.source}>{preview.source || "라이브러리 루트"}</div>
            <div className="text-fg truncate" title={preview.destination}>→ {preview.destination}</div>
            <div className={preview.action === "skip" ? "text-drop mt-1" : preview.conflict === "none" ? "text-keep mt-1" : "text-accent mt-1"}>
              {preview.action === "skip" ? "같은 이름이 있어 실행하지 않습니다" : preview.action === "rename" ? "충돌을 피해 새 이름으로 실행합니다" : "충돌 없음 · 실행 가능"}
            </div>
          </div>
        )}
        {error && <div role="alert" className="mb-3 text-[13px] text-drop">{error}</div>}
        <div className="flex justify-end gap-2">
          <button onClick={onClose} className="h-control px-3 rounded ring-1 ring-line text-fg-dim">취소</button>
          <button
            onClick={execute}
            disabled={busy || !preview || preview.action === "skip"}
            className={`h-control px-3 rounded font-semibold disabled:opacity-40 ${action === "trash" ? "bg-drop text-white" : "bg-accent text-accent-fg"}`}
          >
            {busy ? "작업 중…" : `${labels[action]} 실행`}
          </button>
        </div>
      </div>
    </div>
  );
}
