import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { Btn } from "./ui";
import { fmtBytes, fmtDateTime } from "./format";
import { useConfirm } from "./confirmContext";
import { toast } from "./toastStore";

type Backup = { path: string; name: string; bytes: number; made_at: number };

function Head({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-3 pt-3 pb-1 text-[10.5px] uppercase tracking-wider text-fg-mute">
      {children}
    </div>
  );
}

/**
 * 설정 — 지금은 썸네일 캐시와 앱 정보뿐이다.
 *
 * 다른 갈래와 달리 사진을 거르지 않는다. 레일 아래쪽에 휴지통과 같이 두는
 * 이유다.
 */
export default function SettingsPanel({
  thumbBytes,
  onRefresh,
}: {
  /** 썸네일이 쓰는 용량. 아직 안 셌으면 null */
  thumbBytes: number | null;
  onRefresh: () => void;
}) {
  const ask = useConfirm();
  const [ver, setVer] = useState("");
  const [busy, setBusy] = useState(false);

  /// 백업 목록. 판정·평점·태그 78,857장분이 파일 하나라 잃으면 끝이다.
  const [backups, setBackups] = useState<Backup[]>([]);
  const [backingUp, setBackingUp] = useState(false);
  const [backupMsg, setBackupMsg] = useState("");
  const reloadBackups = useCallback(() => {
    invoke<Backup[]>("db_backups")
      .then(setBackups)
      .catch(() => setBackups([]));
  }, []);
  useEffect(reloadBackups, [reloadBackups]);

  /// 사본으로 되돌린다. 되돌리기 직전 상태도 한 벌 남으니 되돌리기를 되돌릴 수 있다.
  /// 설정까지 바뀌므로 끝나면 화면을 통째로 다시 연다.
  const restore = async (b: Backup) => {
    const ok = await ask({
      title: `${fmtDateTime(b.made_at)} 사본으로 되돌립니다`,
      lines: [
        "· 그 뒤에 한 판정·평점·태그는 사라집니다",
        "· 되돌리기 직전 상태가 한 벌 더 남습니다 — 다시 되돌릴 수 있습니다",
        "· 사진 파일은 건드리지 않습니다",
      ],
      confirmLabel: "이 사본으로",
      danger: true,
    });
    if (!ok) return;
    try {
      await invoke("db_restore", { path: b.path });
      window.location.reload();
    } catch (e) {
      toast(String(e), "drop");
    }
  };

  const backupNow = async () => {
    setBackingUp(true);
    setBackupMsg("");
    try {
      const b = await invoke<Backup>("db_backup");
      setBackupMsg(`${b.name} · ${fmtBytes(b.bytes)}`);
      reloadBackups();
    } catch (e) {
      setBackupMsg(String(e));
    } finally {
      setBackingUp(false);
    }
  };

  useEffect(() => {
    getVersion()
      .then(setVer)
      .catch(() => setVer(""));
  }, []);

  return (
    <>
      <Head>썸네일 캐시</Head>
      <div className="px-3 text-[12px] text-fg-dim tabular-nums">
        {thumbBytes === null ? "—" : fmtBytes(thumbBytes)}
      </div>
      <div className="px-2 pt-2 flex gap-1">
        <Btn onClick={onRefresh}>다시 세기</Btn>
        <Btn
          tone="drop"
          disabled={busy}
          onClick={async () => {
            const ok = await ask({
              title: "썸네일을 모두 지웁니다",
              lines: [
                "· 사진은 그대로입니다",
                "· 다음에 볼 때 다시 만들어집니다 — 12만 장이면 한참 걸립니다",
              ],
              confirmLabel: "비우기",
              danger: true,
            });
            if (!ok) return;
            setBusy(true);
            try {
              await invoke("cache_clear");
              onRefresh();
            } finally {
              setBusy(false);
            }
          }}
        >
          비우기
        </Btn>
      </div>

      <Head>DB 백업</Head>
      <div className="px-3 text-[11.5px] text-fg-mute leading-relaxed">
        판정·평점·태그가 든 파일의 사본입니다. 켤 때 하루 한 벌 저절로 뜨고,
        최신 3벌을 남깁니다.
      </div>
      <div className="px-2 pt-2 flex gap-1">
        <Btn tone="accent" disabled={backingUp} onClick={backupNow}>
          {backingUp ? "만드는 중…" : "지금 백업"}
        </Btn>
        <Btn onClick={() => invoke("db_backups_reveal").catch(() => {})}>
          Finder에서 보기
        </Btn>
      </div>
      {backupMsg && (
        <div className="px-3 pt-1.5 text-[11.5px] text-fg-dim tabular-nums">
          {backupMsg}
        </div>
      )}
      {backups.length > 0 && (
        <div className="px-3 pt-2 space-y-0.5">
          {backups.map((b) => (
            <div
              key={b.name}
              className="group flex items-baseline gap-2 text-[11.5px] tabular-nums"
            >
              <span className="text-fg-dim">{fmtDateTime(b.made_at)}</span>
              <span className="text-fg-faint">{fmtBytes(b.bytes)}</span>
              <button
                onClick={() => restore(b)}
                className="ml-auto hidden group-hover:inline text-drop hover:underline"
              >
                이 사본으로
              </button>
            </div>
          ))}
        </div>
      )}

      <Head>에이컷</Head>
      <div className="px-3 text-[12px] text-fg-dim leading-relaxed">
        버전 {ver || "—"}
        <br />
        <span className="text-fg-mute">사진은 원래 자리에 그대로 둡니다.</span>
      </div>
    </>
  );
}
