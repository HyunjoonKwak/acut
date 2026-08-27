import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  UploadCloud,
  FolderCheck,
  Loader2,
  StopCircle,
  CheckCircle2,
  SkipForward,
  XCircle,
} from "lucide-react";
import { useAppStore } from "@/stores/appStore";
import type {
  NasConfig,
  NasStatus,
  NasUploadProgress,
  NasUploadResult,
} from "@/types";
import { NasConnectPanel } from "./NasConnectPanel";
import { NasFolderBrowser } from "./NasFolderBrowser";

export function NasUploadView() {
  const selectedMediaIds = useAppStore((s) => s.selectedMediaIds);
  const stats = useAppStore((s) => s.stats);
  const triggerRefresh = useAppStore((s) => s.triggerRefresh);

  const [config, setConfig] = useState<NasConfig | null>(null);
  const [status, setStatus] = useState<NasStatus>({
    connected: false,
    account: null,
    url: null,
  });
  const [destRoot, setDestRoot] = useState("");
  // Default to the gallery selection when the view opens with one
  const [scope, setScope] = useState<"all" | "selected">(() =>
    selectedMediaIds.size > 0 ? "selected" : "all"
  );
  const [organizeByDate, setOrganizeByDate] = useState(true);
  const [excludeBcuts, setExcludeBcuts] = useState(true);
  const [skipUploaded, setSkipUploaded] = useState(true);
  const [overwrite, setOverwrite] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [progress, setProgress] = useState<NasUploadProgress | null>(null);
  const [result, setResult] = useState<NasUploadResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const init = async () => {
      try {
        const [cfg, st] = await Promise.all([
          invoke<NasConfig>("nas_get_config"),
          invoke<NasStatus>("nas_status"),
        ]);
        setConfig(cfg);
        setStatus(st);
        if (cfg.dest_root) setDestRoot(cfg.dest_root);
        setOrganizeByDate(cfg.organize_by_date);
      } catch (e) {
        setError(String(e));
      }
    };
    init();
  }, []);

  useEffect(() => {
    const unlisten = listen<NasUploadProgress>("nas-upload-progress", (e) => {
      setProgress(e.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleUpload = async () => {
    setUploading(true);
    setResult(null);
    setError(null);
    setProgress(null);
    try {
      const res = await invoke<NasUploadResult>("nas_upload", {
        mediaIds: scope === "selected" ? Array.from(selectedMediaIds) : null,
        destRoot,
        organizeByDate,
        excludeBcuts,
        skipUploaded,
        overwrite,
      });
      setResult(res);
      triggerRefresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setUploading(false);
    }
  };

  const handleCancel = async () => {
    try {
      await invoke("nas_cancel_upload");
    } catch (e) {
      setError(String(e));
    }
  };

  const canUpload =
    status.connected &&
    !!destRoot &&
    !uploading &&
    (scope === "all" || selectedMediaIds.size > 0);

  const progressPercent =
    progress && progress.total > 0
      ? Math.round((progress.current / progress.total) * 100)
      : 0;

  return (
    <div className="flex h-full overflow-hidden">
      {/* Left: connection + remote folder browser */}
      <aside className="w-80 shrink-0 border-r border-border bg-bg-secondary/40 flex flex-col overflow-hidden">
        <NasConnectPanel
          config={config}
          status={status}
          uploading={uploading}
          onStatusChange={setStatus}
        />
        {status.connected && (
          <NasFolderBrowser destRoot={destRoot} onSelect={setDestRoot} />
        )}
      </aside>

      {/* Right: upload options + progress */}
      <main className="flex-1 overflow-y-auto">
        <div className="max-w-2xl mx-auto p-6">
          <div className="flex items-center gap-2.5 mb-1">
            <UploadCloud size={20} className="text-accent" />
            <h1 className="text-[16px] font-bold text-text-primary">
              NAS 업로드
            </h1>
          </div>
          <p className="text-[11px] text-text-secondary mb-6 leading-relaxed">
            중복 제거와 B컷 컬링을 마친 사진을 Synology NAS로 보냅니다. 업로드된
            사진은 NAS의 사진 앱(nas_photo)에서 이어서 감상·정리할 수 있습니다.
          </p>

          {/* Destination */}
          <section className="mb-5 p-4 rounded-lg border border-border bg-bg-secondary/50">
            <div className="flex items-center gap-2 mb-1.5">
              <FolderCheck size={13} className="text-accent" />
              <span className="text-[11px] font-semibold text-text-primary">
                대상 폴더
              </span>
            </div>
            {destRoot ? (
              <p className="text-[12px] text-text-primary font-medium break-all">
                {destRoot}
                {organizeByDate && (
                  <span className="text-text-secondary font-normal">
                    /YYYY-MM-DD
                  </span>
                )}
              </p>
            ) : (
              <p className="text-[11px] text-text-secondary">
                {status.connected
                  ? "왼쪽에서 업로드할 NAS 폴더를 선택하세요"
                  : "먼저 NAS에 연결하세요"}
              </p>
            )}
          </section>

          {/* Scope */}
          <section className="mb-5">
            <p className="text-[11px] font-semibold text-text-primary mb-2">
              업로드 범위
            </p>
            <div className="flex gap-2">
              <button
                onClick={() => setScope("all")}
                className={`flex-1 p-3 rounded-lg border text-left transition-colors ${
                  scope === "all"
                    ? "border-accent bg-accent/5"
                    : "border-border hover:border-accent/40"
                }`}
              >
                <p className="text-[12px] font-medium text-text-primary">
                  전체 라이브러리
                </p>
                <p className="text-[10px] text-text-secondary mt-0.5">
                  {stats ? `${stats.total_count.toLocaleString()}개 항목` : "—"}
                </p>
              </button>
              <button
                onClick={() => setScope("selected")}
                disabled={selectedMediaIds.size === 0}
                className={`flex-1 p-3 rounded-lg border text-left transition-colors disabled:opacity-40 ${
                  scope === "selected"
                    ? "border-accent bg-accent/5"
                    : "border-border hover:border-accent/40"
                }`}
              >
                <p className="text-[12px] font-medium text-text-primary">
                  갤러리에서 선택한 항목
                </p>
                <p className="text-[10px] text-text-secondary mt-0.5">
                  {selectedMediaIds.size > 0
                    ? `${selectedMediaIds.size.toLocaleString()}개 선택됨`
                    : "갤러리에서 먼저 선택하세요"}
                </p>
              </button>
            </div>
          </section>

          {/* Options */}
          <section className="mb-6 space-y-2">
            <p className="text-[11px] font-semibold text-text-primary mb-2">
              옵션
            </p>
            <OptionCheckbox
              checked={organizeByDate}
              onChange={setOrganizeByDate}
              label="촬영일 기준 날짜 폴더로 정리 (YYYY-MM-DD)"
              hint="EXIF 촬영일이 없으면 파일 수정일을 사용합니다"
            />
            <OptionCheckbox
              checked={excludeBcuts}
              onChange={setExcludeBcuts}
              label="B컷 제외 (A컷만 업로드)"
              hint="B컷 자동 탐지에서 베스트로 선정되지 않은 사진을 제외합니다"
            />
            <OptionCheckbox
              checked={skipUploaded}
              onChange={setSkipUploaded}
              label="이미 업로드한 항목 건너뛰기"
              hint="파일 내용 해시(SHA-256) 기준으로 재업로드를 방지합니다"
            />
            <OptionCheckbox
              checked={overwrite}
              onChange={setOverwrite}
              label="NAS의 같은 이름 파일 덮어쓰기"
              hint="끄면 같은 이름 파일이 있을 때 건너뜁니다"
            />
          </section>

          {/* Action */}
          {!uploading ? (
            <button
              onClick={handleUpload}
              disabled={!canUpload}
              className="w-full flex items-center justify-center gap-2 h-9 rounded-lg text-[12px] font-semibold bg-accent text-white hover:bg-accent-hover disabled:opacity-40 transition-colors"
            >
              <UploadCloud size={14} />
              업로드 시작
            </button>
          ) : (
            <div className="p-4 rounded-lg border border-accent/30 bg-accent/5">
              <div className="flex items-center justify-between mb-2">
                <span className="flex items-center gap-2 text-[11px] font-medium text-accent">
                  <Loader2 size={12} className="animate-spin" />
                  업로드 중... {progress ? `${progress.current}/${progress.total}` : ""}
                </span>
                <button
                  onClick={handleCancel}
                  className="flex items-center gap-1 px-2 py-1 rounded-md text-[10px] text-danger border border-danger/30 hover:bg-danger/10 transition-colors"
                >
                  <StopCircle size={11} />
                  취소
                </button>
              </div>
              <div className="h-1.5 bg-bg-secondary rounded-full overflow-hidden mb-2">
                <div
                  className="h-full bg-accent rounded-full transition-all"
                  style={{ width: `${progressPercent}%` }}
                />
              </div>
              <p className="text-[10px] text-text-secondary truncate">
                {progress?.current_file}
              </p>
              {progress && (
                <p className="text-[10px] text-text-secondary mt-1 tabular-nums">
                  성공 {progress.uploaded} · 건너뜀 {progress.skipped} · 실패{" "}
                  {progress.failed}
                </p>
              )}
            </div>
          )}

          {error && (
            <p className="mt-3 text-[11px] text-danger break-all leading-relaxed">
              {error}
            </p>
          )}

          {/* Result */}
          {result && !uploading && (
            <section className="mt-5 p-4 rounded-lg border border-border bg-bg-secondary/50">
              <p className="text-[11px] font-semibold text-text-primary mb-3">
                {result.cancelled ? "업로드 취소됨" : "업로드 완료"}
              </p>
              <div className="flex gap-4 mb-2">
                <span className="flex items-center gap-1.5 text-[11px] text-success">
                  <CheckCircle2 size={12} />
                  성공 {result.uploaded.toLocaleString()}
                </span>
                <span className="flex items-center gap-1.5 text-[11px] text-text-secondary">
                  <SkipForward size={12} />
                  건너뜀 {result.skipped.toLocaleString()}
                </span>
                <span className="flex items-center gap-1.5 text-[11px] text-danger">
                  <XCircle size={12} />
                  실패 {result.failed.length.toLocaleString()}
                </span>
              </div>
              {result.ledger_failures > 0 && (
                <p className="text-[10px] text-warning leading-relaxed">
                  {result.ledger_failures}건은 업로드는 성공했지만 로컬 기록에
                  실패했습니다. 다음 업로드에서 건너뛰기가 동작하지 않을 수
                  있습니다.
                </p>
              )}
              {result.failed.length > 0 && (
                <div className="mt-2 max-h-40 overflow-y-auto space-y-1 border-t border-border pt-2">
                  {result.failed.map((f, i) => (
                    <p
                      key={`${f.file_name}-${i}`}
                      className="text-[10px] text-text-secondary break-all"
                    >
                      <span className="text-danger">{f.file_name}</span> —{" "}
                      {f.error}
                    </p>
                  ))}
                </div>
              )}
            </section>
          )}
        </div>
      </main>
    </div>
  );
}

function OptionCheckbox({
  checked,
  onChange,
  label,
  hint,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  hint: string;
}) {
  return (
    <label className="flex items-start gap-2.5 p-2.5 rounded-lg border border-border hover:border-accent/30 cursor-pointer transition-colors">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="mt-0.5 accent-[var(--color-accent)]"
      />
      <span>
        <span className="block text-[11px] font-medium text-text-primary">
          {label}
        </span>
        <span className="block text-[10px] text-text-secondary mt-0.5">
          {hint}
        </span>
      </span>
    </label>
  );
}
