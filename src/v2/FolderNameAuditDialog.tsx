import { useEffect, useId, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type AuditItem = {
  source_dir: string;
  parent_dir: string;
  current_name: string;
  proposed_name: string;
  reason: string;
  file_count: number;
  conflict: boolean;
};

type ApplyOutcome = {
  batch_id: number;
  completed: number;
  failed: number;
  conflicts: number;
  first_error: string | null;
};

export default function FolderNameAuditDialog({
  libraryId,
  libraryName,
  onChanged,
  onClose,
}: {
  libraryId: number;
  libraryName: string;
  onChanged: () => void | Promise<void>;
  onClose: () => void;
}) {
  const titleId = useId();
  const [items, setItems] = useState<AuditItem[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = async () => {
    setBusy(true);
    setError(null);
    try {
      const found = await invoke<AuditItem[]>("folder_name_audit", {
        libraryId,
      });
      setItems(found);
      setSelected(
        new Set(
          found.filter((item) => !item.conflict).map((item) => item.source_dir),
        ),
      );
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void load();
    // libraryId가 바뀌면 다이얼로그가 새로 만들어진다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [libraryId]);

  useEffect(() => {
    const key = (event: KeyboardEvent) => event.key === "Escape" && onClose();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onClose]);

  const apply = async () => {
    if (selected.size === 0) return;
    setBusy(true);
    setError(null);
    try {
      const result = await invoke<ApplyOutcome>("folder_name_apply", {
        libraryId,
        sourceDirs: [...selected],
      });
      if (result.completed === 0 || result.failed > 0) {
        setError(
          `${result.completed}개 변경 · ${result.failed}개 건너뜀${
            result.first_error ? ` — ${result.first_error}` : ""
          }`,
        );
        await load();
        if (result.completed > 0) await onChanged();
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

  return (
    <div className="fixed inset-0 z-[68] bg-canvas/95 backdrop-blur-sm flex items-center justify-center p-6">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="w-[760px] max-w-full max-h-[86vh] flex flex-col bg-chrome rounded-xl ring-1 ring-line shadow-2xl"
      >
        <div className="p-5 pb-3">
          <h2 id={titleId} className="text-[16px] font-semibold text-fg">
            폴더 이름 감사
          </h2>
          <p className="mt-1 text-[12.5px] text-fg-mute">
            {libraryName} · 실제 변경 전 미리보기입니다. 제목은 보존하고 날짜만
            YYYY-MM-DD로 통일합니다.
          </p>
        </div>

        <div className="min-h-0 flex-1 overflow-auto px-5">
          {!busy && items.length === 0 && !error && (
            <div className="rounded bg-raised px-3 py-5 text-center text-[13px] text-fg-mute">
              바꿀 날짜 폴더가 없습니다.
            </div>
          )}
          {items.map((item) => (
            <label
              key={item.source_dir}
              className={`mb-2 flex items-start gap-3 rounded-lg p-3 ring-1 ${
                item.conflict
                  ? "bg-drop/5 ring-drop/40"
                  : "bg-raised ring-line"
              }`}
            >
              <input
                type="checkbox"
                checked={selected.has(item.source_dir)}
                disabled={busy || item.conflict}
                onChange={(event) => {
                  const next = new Set(selected);
                  if (event.target.checked) next.add(item.source_dir);
                  else next.delete(item.source_dir);
                  setSelected(next);
                }}
                className="mt-1 accent-accent"
              />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13px] text-fg-dim line-through decoration-fg-faint">
                  {item.source_dir}
                </span>
                <span className="block truncate text-[14px] font-medium text-accent">
                  → {item.parent_dir ? `${item.parent_dir}/` : ""}
                  {item.proposed_name}
                </span>
                <span className="mt-1 block text-[12px] text-fg-mute">
                  {item.reason} · 사진 {item.file_count.toLocaleString()}장
                  {item.conflict && " · 같은 이름이 있어 자동 적용하지 않습니다"}
                </span>
              </span>
            </label>
          ))}
        </div>

        <div className="p-5 pt-3">
          {error && <div className="mb-3 text-[13px] text-drop">{error}</div>}
          <div className="flex items-center gap-2">
            <button
              onClick={() => void apply()}
              disabled={busy || selected.size === 0}
              className="h-control rounded-lg bg-accent px-3.5 text-[14px] font-semibold text-accent-fg disabled:opacity-40"
            >
              {busy ? "확인 중…" : `선택 ${selected.size.toLocaleString()}개 적용`}
            </button>
            <button
              onClick={onClose}
              className="h-control rounded-lg px-3 text-[14px] text-fg-dim ring-1 ring-line-strong"
            >
              닫기
            </button>
            <div className="flex-1" />
            <span className="text-[12px] text-fg-mute">한 번에 되돌릴 수 있습니다</span>
          </div>
        </div>
      </div>
    </div>
  );
}
