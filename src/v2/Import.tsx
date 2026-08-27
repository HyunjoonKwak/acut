import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Btn } from "./ui";
import { fmtBytes } from "./format";

type Preview = {
  files: number;
  bytes: number;
  duplicates: number;
  days: string[];
  day_count: number;
};

type Progress = {
  found: number;
  copied: number;
  skipped: number;
  failed: number;
  current: string;
};

type Report = {
  copied: number;
  skipped: number;
  failed: number;
  bytes: number;
  first_error: string | null;
};

type Library = { id: number; name: string; online: boolean };

/**
 * 가져오기 — 카드나 다른 폴더의 사진을 라이브러리로 들인다.
 *
 * 무엇이 몇 장 어디로 들어가는지 먼저 보여 준다. 파일을 실제로 만드는
 * 일이라 「눌러 보고 알기」로 둘 수 없다.
 */
export default function Import({
  libs,
  libId,
  onDone,
  onClose,
}: {
  libs: Library[];
  /** 지금 보고 있는 라이브러리. 들어갈 곳의 첫 후보다. */
  libId: number | null;
  onDone: () => void;
  onClose: () => void;
}) {
  const usable = libs.filter((l) => l.online);
  const [source, setSource] = useState<string | null>(null);
  const [target, setTarget] = useState<number | null>(
    libId ?? usable[0]?.id ?? null,
  );
  const [preview, setPreview] = useState<Preview | null>(null);
  const [looking, setLooking] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [error, setError] = useState("");

  const look = useCallback(async (src: string, lib: number) => {
    setLooking(true);
    setPreview(null);
    setError("");
    try {
      setPreview(
        await invoke<Preview>("import_preview", {
          source: src,
          libraryId: lib,
        }),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setLooking(false);
    }
  }, []);

  const choose = async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    setSource(picked);
    if (target !== null) look(picked, target);
  };

  // 들어갈 곳을 바꾸면 겹치는 것의 수가 달라진다 — 다시 센다
  useEffect(() => {
    if (source && target !== null) look(source, target);
  }, [source, target, look]);

  useEffect(() => {
    const un = [
      listen<Progress>("import-progress", (e) => setProgress(e.payload)),
      listen<Report>("import-done", (e) => {
        setReport(e.payload);
        setProgress(null);
        onDone();
      }),
    ];
    return () => {
      un.forEach((p) => p.then((f) => f()));
    };
  }, [onDone]);

  const run = async () => {
    if (!source || target === null) return;
    setReport(null);
    setError("");
    setProgress({ found: 0, copied: 0, skipped: 0, failed: 0, current: "" });
    try {
      await invoke("import_run", { source, libraryId: target });
    } catch (e) {
      setError(String(e));
      setProgress(null);
    }
  };

  const running = progress !== null;

  return (
    <div className="fixed inset-0 z-[80] flex items-center justify-center bg-black/50">
      <div className="w-[520px] max-w-[92vw] rounded-xl bg-chrome ring-1 ring-line-strong shadow-2xl p-5">
        <div className="flex items-baseline gap-3">
          <span className="text-[15px] font-semibold text-fg">가져오기</span>
          <span className="text-[11.5px] text-fg-mute">
            복사만 합니다 — 원본은 그대로 둡니다
          </span>
        </div>

        {/* 어디서 */}
        <Label>어디서</Label>
        <div className="flex items-center gap-2">
          <div className="flex-1 min-w-0 h-control px-2 rounded-md bg-canvas ring-1 ring-line flex items-center">
            <span className="text-[12px] truncate" title={source ?? undefined}>
              {source ?? <span className="text-fg-faint">폴더를 고르세요</span>}
            </span>
          </div>
          <Btn onClick={choose} disabled={running}>
            고르기…
          </Btn>
        </div>

        {/* 어디로 */}
        <Label>어디로</Label>
        {usable.length === 0 ? (
          <div className="text-[12px] text-drop">
            연결된 라이브러리가 없습니다.
          </div>
        ) : (
          <div className="flex flex-wrap gap-1">
            {usable.map((l) => (
              <button
                key={l.id}
                disabled={running}
                onClick={() => setTarget(l.id)}
                className={`h-control px-2.5 rounded-md text-[12px] disabled:opacity-40 ${
                  target === l.id
                    ? "bg-accent text-accent-fg"
                    : "bg-raised text-fg-dim hover:text-fg"
                }`}
              >
                {l.name}
              </button>
            ))}
          </div>
        )}

        {/* 무엇이 들어가나 */}
        {looking && (
          <div className="mt-4 text-[12px] text-fg-mute">세는 중…</div>
        )}
        {preview && !running && !report && (
          <div className="mt-4 rounded-lg bg-raised p-3">
            {preview.files === 0 ? (
              <div className="text-[12.5px] text-fg-dim">
                새로 들일 사진이 없습니다.
                {preview.duplicates > 0 &&
                  ` ${preview.duplicates.toLocaleString()}장은 이미 있습니다.`}
              </div>
            ) : (
              <>
                <div className="text-[13px] text-fg">
                  <b className="tabular-nums">
                    {preview.files.toLocaleString()}장
                  </b>{" "}
                  <span className="text-fg-mute tabular-nums">
                    · {fmtBytes(preview.bytes)}
                  </span>
                </div>
                <div className="mt-1.5 text-[11.5px] text-fg-mute leading-relaxed">
                  찍은 날짜로 갈라 넣습니다 ({preview.day_count}일치)
                  <br />
                  <span className="text-fg-faint font-mono">
                    {preview.days
                      .map((d) => `${d.slice(0, 4)}/${d}`)
                      .join(" · ")}
                    {preview.day_count > preview.days.length && " …"}
                  </span>
                </div>
                {preview.duplicates > 0 && (
                  <div className="mt-1.5 text-[11.5px] text-fg-mute">
                    이미 있는 {preview.duplicates.toLocaleString()}장은
                    건너뜁니다
                  </div>
                )}
              </>
            )}
          </div>
        )}

        {/* 하는 중 */}
        {progress && (
          <div className="mt-4">
            <div className="flex items-baseline gap-2 text-[12.5px]">
              <span className="text-fg tabular-nums">
                {progress.copied.toLocaleString()} /{" "}
                {progress.found.toLocaleString()}
              </span>
              <span className="text-fg-mute truncate flex-1">
                {progress.current}
              </span>
            </div>
            <div className="mt-1.5 h-1 rounded-full bg-raised overflow-hidden">
              <div
                className="h-full bg-accent transition-[width] duration-150"
                style={{
                  width: `${
                    progress.found === 0
                      ? 0
                      : ((progress.copied + progress.skipped) /
                          progress.found) *
                        100
                  }%`,
                }}
              />
            </div>
          </div>
        )}

        {/* 끝났다 */}
        {report && (
          <div className="mt-4 rounded-lg bg-raised p-3 text-[12.5px]">
            <div className="text-fg">
              {report.copied.toLocaleString()}장 들였습니다{" "}
              <span className="text-fg-mute tabular-nums">
                · {fmtBytes(report.bytes)}
              </span>
            </div>
            <div className="mt-1 text-[11.5px] text-fg-mute">
              {report.skipped > 0 &&
                `이미 있던 ${report.skipped.toLocaleString()}장은 건너뛰었습니다. `}
              썸네일은 뒤에서 계속 만들어집니다.
            </div>
            {report.failed > 0 && (
              <div className="mt-1 text-[11.5px] text-drop">
                {report.failed.toLocaleString()}장 실패 — {report.first_error}
              </div>
            )}
          </div>
        )}

        {error && <div className="mt-3 text-[12px] text-drop">{error}</div>}

        <div className="mt-5 flex justify-end gap-2">
          <Btn onClick={onClose} disabled={running}>
            {report ? "닫기" : "취소"}
          </Btn>
          {!report && (
            <Btn
              tone="accent"
              disabled={
                running || !source || target === null || !preview?.files
              }
              onClick={run}
            >
              {running ? "가져오는 중…" : "가져오기"}
            </Btn>
          )}
        </div>
      </div>
    </div>
  );
}

function Label({ children }: { children: React.ReactNode }) {
  return (
    <div className="mt-4 mb-1.5 text-[10.5px] uppercase tracking-wider text-fg-mute">
      {children}
    </div>
  );
}
