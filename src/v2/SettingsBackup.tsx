import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes, fmtDateTime } from "./format";
import { useJob } from "./jobStore";
import { toast } from "./toastStore";
import { Btn } from "./ui";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Section, Row } from "./settingsUi";

type BackupTarget = {
  target: { uuid: string; rel: string; name: string } | null;
  online: boolean;
  dir: string | null;
  free_bytes: number | null;
  last: {
    at: number;
    copied: number;
    updated: number;
    bytes: number;
    errors: number;
    cancelled: boolean;
  } | null;
};
type BackupPlan = {
  libs: {
    library_id: number;
    name: string;
    files: number;
    bytes: number;
    conflicts: number;
    orphans: number;
    offline: boolean;
  }[];
  files: number;
  bytes: number;
  conflicts: number;
  orphans: number;
};

/**
 * 백업 — 라이브러리를 다른 디스크에 한 벌 더 (RAW 한 벌의 보험).
 *
 * NAS는 Drive Client가 맞추고, 로컬 디스크끼리는 여기서. 한 방향이다:
 * 백업에만 있는 파일은 세어 알릴 뿐 지우지 않는다.
 */

export function Backup() {
  const [t, setT] = useState<BackupTarget | null>(null);
  const [plan, setPlan] = useState<BackupPlan | null>(null);
  const [planning, setPlanning] = useState(false);
  const job = useJob((s) => s.job);
  const busy = job !== null;
  const backing = job?.label === "백업";

  const reload = useCallback(() => {
    invoke<BackupTarget>("backup_target")
      .then(setT)
      .catch(() => setT(null));
  }, []);
  useEffect(() => {
    reload();
    if (!busy) return;
    const i = setInterval(reload, 5_000);
    return () => clearInterval(i);
  }, [reload, busy]);

  const choose = async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    try {
      setT(await invoke<BackupTarget>("backup_set_target", { path: picked }));
      setPlan(null);
    } catch (e) {
      toast(String(e), "drop");
    }
  };
  const look = async () => {
    setPlanning(true);
    try {
      setPlan(await invoke<BackupPlan>("backup_plan"));
    } catch (e) {
      toast(String(e), "drop");
    } finally {
      setPlanning(false);
    }
  };
  const run = async () => {
    try {
      await invoke("backup_run");
      setPlan(null);
    } catch (e) {
      toast(String(e), "drop");
    }
  };
  const stop = () => invoke("scan_cancel").catch(() => {});

  const lastLine = t?.last
    ? `마지막 백업 ${fmtDateTime(t.last.at)} — ${(t.last.copied + t.last.updated).toLocaleString()}장 · ${fmtBytes(t.last.bytes)}${t.last.errors ? ` · 문제 ${t.last.errors}건` : ""}${t.last.cancelled ? " · 멈춤" : ""}`
    : "아직 백업한 적이 없습니다.";

  return (
    <Section id="backup" title="백업">
      <Row
        label="백업 디스크"
        hint={
          !t
            ? "…"
            : !t.target
              ? "라이브러리 전부를 한 벌 더 복사할 다른 디스크의 폴더. RAW도 갑니다 — 맥에 한 벌뿐이니까요."
              : t.online
                ? `${t.dir} — 남은 ${t.free_bytes === null ? "?" : fmtBytes(t.free_bytes)}. ${lastLine}`
                : `「${t.target.name}」 디스크가 연결되어 있지 않습니다. ${lastLine}`
        }
      >
        <Btn onClick={choose} disabled={busy}>
          {t?.target ? "바꾸기…" : "고르기…"}
        </Btn>
      </Row>
      <Row
        label="지금 백업"
        hint={
          backing
            ? `${job.done.toLocaleString()} / ${job.total.toLocaleString()}장 복사 중. 멈춰도 한 것은 남습니다.`
            : plan
              ? plan.files === 0
                ? `새로 복사할 것이 없습니다.${plan.conflicts ? ` 백업 쪽이 더 새것인 파일 ${plan.conflicts}개는 건너뜁니다.` : ""}${plan.orphans ? ` 백업에만 있는 파일 ${plan.orphans}개는 그대로 둡니다.` : ""}`
                : `${plan.files.toLocaleString()}장 · ${fmtBytes(plan.bytes)} 복사합니다.${plan.conflicts ? ` 백업 쪽이 더 새것인 ${plan.conflicts}개는 건너뜁니다.` : ""}${plan.orphans ? ` 백업에만 있는 ${plan.orphans}개는 그대로 둡니다.` : ""}${
                    plan.libs.some((l) => l.offline)
                      ? ` 연결 안 된 라이브러리: ${plan.libs
                          .filter((l) => l.offline)
                          .map((l) => l.name)
                          .join(", ")}.`
                      : ""
                  }`
              : "먼저 「살펴보기」 — 무엇이 얼마나 복사될지 보여 드립니다. 원본보다 새로운 백업은 건드리지 않고, 복사한 파일은 다시 읽어 맞는지 봅니다."
        }
      >
        {backing ? (
          <Btn tone="drop" onClick={stop}>
            멈추기
          </Btn>
        ) : plan && plan.files > 0 ? (
          <Btn tone="accent" disabled={busy} onClick={run}>
            백업 시작
          </Btn>
        ) : (
          <Btn
            tone="accent"
            disabled={busy || planning || !t?.online}
            onClick={look}
          >
            {planning ? "살펴보는 중…" : "살펴보기"}
          </Btn>
        )}
      </Row>
      {plan && plan.libs.length > 0 && (
        <div className="px-4 pb-3 text-[12.5px] text-fg-mute tabular-nums">
          {plan.libs.map((l) => (
            <div key={l.library_id}>
              {l.name} —{" "}
              {l.offline
                ? "연결 안 됨"
                : `${l.files.toLocaleString()}장 · ${fmtBytes(l.bytes)}`}
            </div>
          ))}
        </div>
      )}
    </Section>
  );
}

