import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useConfirm } from "./confirmContext";
import { useData } from "./dataStore";
import { fmtBytes, fmtDateTime } from "./format";
import { toast } from "./toastStore";
import { Btn } from "./ui";
import { Section, Row } from "./settingsUi";

type Backup = { path: string; name: string; bytes: number; made_at: number };

export function Database() {
  const ask = useConfirm();
  const cache = useData((s) => s.cache);
  const refreshCache = useData((s) => s.refreshCache);
  const [info, setInfo] = useState<{ path: string; bytes: number } | null>(
    null,
  );
  const [backups, setBackups] = useState<Backup[]>([]);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(() => {
    invoke<{ path: string; bytes: number }>("db_info")
      .then(setInfo)
      .catch(() => setInfo(null));
    invoke<Backup[]>("db_backups")
      .then(setBackups)
      .catch(() => setBackups([]));
  }, []);
  useEffect(reload, [reload]);

  const backupNow = async () => {
    setBusy(true);
    try {
      const b = await invoke<Backup>("db_backup");
      toast(`백업 — ${b.name} · ${fmtBytes(b.bytes)}`, "ok");
      reload();
    } catch (e) {
      toast(String(e), "drop");
    } finally {
      setBusy(false);
    }
  };
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
  const clearCache = async () => {
    const ok = await ask({
      title: "썸네일을 모두 지웁니다",
      lines: [
        "· 사진은 그대로입니다",
        "· 다음에 볼 때 다시 만들어집니다 — 8만 장이면 몇 분 걸립니다",
      ],
      confirmLabel: "비우기",
      danger: true,
    });
    if (!ok) return;
    try {
      await invoke("cache_clear");
      refreshCache();
      toast("썸네일을 비웠습니다");
    } catch (e) {
      toast(String(e), "drop");
    }
  };

  return (
    <Section id="database" title="데이터베이스">
      <Row label="지금 쓰는 파일" hint={info?.path ?? "—"}>
        <span className="text-[12.5px] text-fg-mute tabular-nums">
          {info ? fmtBytes(info.bytes) : "—"}
        </span>
        <Btn onClick={() => invoke("db_backups_reveal").catch(() => {})}>
          Finder에서 보기
        </Btn>
      </Row>
      <Row
        label="백업"
        hint="켤 때 하루 한 벌 저절로 뜨고 최신 3벌을 남깁니다. 판정·평점·태그가 든 파일의 사본입니다."
      >
        <Btn tone="accent" disabled={busy} onClick={backupNow}>
          {busy ? "만드는 중…" : "지금 백업"}
        </Btn>
      </Row>
      {backups.length > 0 && (
        <div className="px-4 py-2 space-y-1">
          {backups.map((b) => (
            <div
              key={b.name}
              className="group flex items-baseline gap-3 text-[12px] tabular-nums"
            >
              <span className="text-fg-dim">{fmtDateTime(b.made_at)}</span>
              <span className="text-fg-faint">{fmtBytes(b.bytes)}</span>
              <button
                onClick={() => restore(b)}
                className="ml-auto text-drop opacity-0 group-hover:opacity-100 hover:underline"
              >
                이 사본으로 되돌리기
              </button>
            </div>
          ))}
        </div>
      )}
      <Row
        label="썸네일 캐시"
        hint="사진을 다시 읽지 않으려고 만들어 둔 작은 그림들"
      >
        <span className="text-[12.5px] text-fg-mute tabular-nums">
          {cache ? fmtBytes(cache.bytes) : "—"}
        </span>
        <Btn onClick={refreshCache}>다시 세기</Btn>
        <Btn tone="drop" onClick={clearCache}>
          비우기
        </Btn>
      </Row>
    </Section>
  );
}

