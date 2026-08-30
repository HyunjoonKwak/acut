import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useJob } from "./jobStore";

/** 도는 동안 다른 일을 막아야 하는 작업 — 파일이 실제로 움직이는 것들 */
const BLOCKING = new Set(["폴더 합치는 중", "옮기는 중"]);

/**
 * 막는 진행 창 — 폴더 합치기처럼 파일이 움직이는 동안 화면 전체를 덮고 진행률만 보여 준다.
 * 다른 단추·키가 눌리지 않게(겹쳐 돌면 폴더 행이 사라지거나 이름이 부딪힌다 — 실측 2026-08-30).
 */
export default function BlockingJob() {
  const job = useJob((s) => s.job);
  const open = !!job && BLOCKING.has(job.label);

  // 키보드도 막는다 — 격자 단축키가 뒤에서 찍히지 않게
  useEffect(() => {
    if (!open) return;
    const eat = (e: KeyboardEvent) => {
      e.stopPropagation();
      e.preventDefault();
    };
    window.addEventListener("keydown", eat, true);
    return () => window.removeEventListener("keydown", eat, true);
  }, [open]);

  if (!open || !job) return null;
  const pct = job.total > 0 ? Math.min(100, Math.round((job.done / job.total) * 100)) : 0;
  return (
    <div className="fixed inset-0 z-[80] bg-black/55 flex items-center justify-center" onMouseDown={(e) => e.stopPropagation()}>
      <div className="w-[420px] rounded-xl bg-chrome ring-1 ring-line-strong shadow-2xl p-5">
        <div className="flex items-center gap-2 text-[14px] font-semibold text-fg">
          <i className="w-2.5 h-2.5 rounded-full bg-keep animate-pulse" />
          {job.label}…
        </div>
        <div className="mt-3 h-2.5 rounded-full bg-raised overflow-hidden">
          <div className="h-full bg-keep transition-[width] duration-200" style={{ width: `${pct}%` }} />
        </div>
        <div className="mt-2 flex items-baseline justify-between text-[12.5px] tabular-nums">
          <span className="text-fg-dim">
            {job.done.toLocaleString()} / {job.total.toLocaleString()}장
          </span>
          <span className="text-fg font-semibold">{pct}%</span>
        </div>
        <div className="mt-3 text-[12px] text-fg-mute">
          끝날 때까지 다른 일은 할 수 없습니다. 멈추면 옮긴 것은 그대로 두고 ⌘Z 로 되돌릴 수 있습니다.
        </div>
        <div className="mt-4 flex justify-end">
          <button
            onClick={() => void invoke("scan_cancel")}
            className="h-control px-3 rounded-md text-drop ring-1 ring-drop/50 hover:bg-drop/10 text-[12.5px]"
          >
            멈추기
          </button>
        </div>
      </div>
    </div>
  );
}
