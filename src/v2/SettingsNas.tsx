import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useConfirm } from "./confirmContext";
import { useData } from "./dataStore";
import { fmtBytes } from "./format";
import { useJob } from "./jobStore";
import { usePref } from "./prefs";
import { toast } from "./toastStore";
import { Btn } from "./ui";
import { areaLabel } from "./areaItems";
import { Section, Row } from "./settingsUi";

type NasConfig = {
  host: string;
  zone1: string;
  photos: string;
  shared: string;
  exclude: string;
  rsync_port: number;
};
type NasStatus = {
  online: boolean;
  hostname: string;
  free_bytes: number | null;
  zone1_files: number | null;
  error: string | null;
  rsync: string;
  rsync_ok: boolean;
};
type Verified = {
  library_id: number;
  present: number;
  missing: number;
  sample: string[];
};
type PurgePlan = {
  items: { rel: string; size: number; why: string }[];
  bytes: number;
  pending: number;
  unknown: number;
};

/**
 * NAS — 종(從)이다. 동기화는 Drive Client가, Photo Desk은 1차 구역을 내려받고,
 * 올라갔는지 확인하고, 확인된 것만 1차에서 비운다. 순서가 곧 안전장치.
 */
export function Nas() {
  const ask = useConfirm();
  const libs = useData((s) => s.libs);
  const job = useJob((s) => s.job);
  const busy = job !== null;
  const [nasAuto, setNasAuto] = usePref("nasAuto");
  const [cfg, setCfg] = useState<NasConfig | null>(null);
  const [dirty, setDirty] = useState(false);
  const [st, setSt] = useState<NasStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [verified, setVerified] = useState<Record<number, Verified>>({});
  const [verifying, setVerifying] = useState<number | null>(null);
  const [plan, setPlan] = useState<PurgePlan | null>(null);
  const [purging, setPurging] = useState(false);
  const desks = libs.filter((l) => l.area === 0 && l.online);
  const mirrored = libs.filter(
    (l) => (l.area === 1 || l.area === 2) && l.online,
  );

  useEffect(() => {
    invoke<NasConfig>("nas_config")
      .then(setCfg)
      .catch(() => setCfg(null));
  }, []);

  const save = async () => {
    if (!cfg) return;
    try {
      setCfg(await invoke<NasConfig>("nas_config_set", { config: cfg }));
      setDirty(false);
      toast("NAS 설정을 저장했습니다", "ok");
    } catch (e) {
      toast(String(e), "drop");
    }
  };
  const check = async () => {
    setChecking(true);
    try {
      const r = await invoke<NasStatus>("nas_check");
      setSt(r);
      // 툴바의 NAS 불도 같이 — 여기서 «연결됨»인데 불이 빨간 채로 남지 않게
      useData.getState().setNasStatus({
        online: r.online,
        hostname: r.hostname,
        error: r.error ?? null,
        at: Math.floor(Date.now() / 1000),
      });
      toast(
        r.online
          ? `NAS 연결됨 — ${r.hostname}${r.rsync_ok ? "" : " · rsync는 못 씀"}`
          : `NAS 연결 실패 — ${r.error ?? ""}`,
        r.online && r.rsync_ok ? "ok" : "drop",
      );
    } catch (e) {
      toast(String(e), "drop");
    } finally {
      setChecking(false);
    }
  };
  const pull = async (libraryId: number) => {
    const lib = libs.find((l) => l.id === libraryId);
    const ok = await ask({
      title: `NAS 1차 구역을 「${lib?.name}」의 NAS-1차/로 내려받습니다`,
      lines: [
        "· 새로 생긴 것과 바뀐 것만 받습니다 (rsync). 끊겨도 이어받습니다",
        "· NAS 쪽은 건드리지 않습니다",
        "· 끝나면 그 라이브러리를 스캔합니다",
      ],
      confirmLabel: "내려받기",
    });
    if (!ok) return;
    try {
      await invoke("nas_pull_start", { libraryId });
    } catch (e) {
      toast(String(e), "drop");
    }
  };
  const verify = async (libraryId: number) => {
    setVerifying(libraryId);
    try {
      const v = await invoke<Verified>("nas_verify", { libraryId });
      setVerified((m) => ({ ...m, [libraryId]: v }));
    } catch (e) {
      toast(String(e), "drop");
    } finally {
      setVerifying(null);
    }
  };
  const lookPurge = async (libraryId: number) => {
    try {
      setPlan(await invoke<PurgePlan>("nas_purge_plan", { libraryId }));
    } catch (e) {
      toast(String(e), "drop");
    }
  };
  const runPurge = async () => {
    if (!plan || plan.items.length === 0) return;
    const ok = await ask({
      title: `NAS 1차 구역에서 ${plan.items.length.toLocaleString()}개(${fmtBytes(plan.bytes)})를 #trash로 옮깁니다`,
      lines: [
        "· 우리가 내려받았고, 작업대에서 정리돼 NAS에 올라간 것이 확인됐거나 버린 것만",
        "· 지우지 않고 1차 구역 안의 #trash/ 폴더로 옮깁니다 — 거기서 지우는 건 사람이",
        `· 아직 작업대에 있는 ${plan.pending.toLocaleString()}개와 행방을 모르는 ${plan.unknown.toLocaleString()}개는 건드리지 않습니다`,
      ],
      confirmLabel: "#trash로",
      danger: true,
    });
    if (!ok) return;
    setPurging(true);
    try {
      await invoke("nas_purge_run", { rels: plan.items.map((i) => i.rel) });
      setPlan(null);
    } catch (e) {
      toast(String(e), "drop");
    } finally {
      setPurging(false);
    }
  };

  const field = (k: keyof NasConfig, label: string) => (
    <label key={k} className="flex items-center gap-2 text-[13px]">
      <span className="w-20 shrink-0 text-fg-mute">{label}</span>
      <input
        value={cfg?.[k] ?? ""}
        onChange={(e) => {
          setCfg((c) => (c ? { ...c, [k]: e.target.value } : c));
          setDirty(true);
        }}
        className="flex-1 min-w-0 h-7 px-2 rounded bg-canvas text-[13px] text-fg ring-1 ring-line outline-none focus:ring-accent font-mono"
      />
    </label>
  );

  return (
    <Section id="nas" title="NAS">
      <Row
        label="연결"
        hint={
          !st
            ? "ssh 설정의 Host 이름으로 붙습니다 (포트·키는 ~/.ssh/config). 자격증명은 저장하지 않습니다. 내려받기·확인은 NAS의 rsync를 쓰므로 DSM 제어판 › 파일 서비스 › rsync를 켜 두어야 합니다."
            : st.online
              ? `${st.hostname} — 남은 ${st.free_bytes === null ? "?" : fmtBytes(st.free_bytes)}, 1차 구역 파일 ${st.zone1_files?.toLocaleString() ?? "?"}개`
              : `연결 실패 — ${st.error ?? ""}`
        }
      >
        <Btn onClick={check} disabled={checking}>
          {checking ? "확인 중…" : "연결 확인"}
        </Btn>
      </Row>
      {st && (
        <div className="mx-4 mb-3 px-3 py-2 rounded-md bg-raised text-[13px] flex items-start gap-2">
          <span
            className="mt-1.5 w-2 h-2 rounded-full shrink-0"
            style={{
              background:
                st.online && st.rsync_ok
                  ? "var(--color-ok)"
                  : "var(--color-drop)",
            }}
          />
          <div className="min-w-0">
            <div className="text-fg font-semibold">
              {st.online ? `연결됨 — ${st.hostname}` : "연결 실패"}
            </div>
            <div className="text-fg-dim">
              {st.online
                ? `남은 공간 ${st.free_bytes === null ? "?" : fmtBytes(st.free_bytes)} · 1차 구역 파일 ${st.zone1_files?.toLocaleString() ?? "?"}개`
                : st.error}
            </div>
            <div className={st.rsync_ok ? "text-fg-mute" : "text-drop"}>
              {st.rsync_ok
                ? st.rsync
                : `${st.rsync} — macOS 내장 openrsync는 못 씁니다. 터미널에서 brew install rsync`}
            </div>
          </div>
        </div>
      )}
      <div className="px-4 pb-3 flex flex-col gap-1.5">
        {field("host", "호스트")}
        {field("zone1", "1차 구역")}
        {field("photos", "개인(내사진)")}
        {field("shared", "공용")}
        {field("exclude", "제외")}
        <label className="flex items-center gap-2 text-[13px]">
          <span className="w-20 shrink-0 text-fg-mute">rsync 포트</span>
          <input
            value={cfg?.rsync_port ?? 22}
            onChange={(e) => {
              const n = Number(e.target.value);
              setCfg((c) =>
                c ? { ...c, rsync_port: Number.isFinite(n) ? n : 22 } : c,
              );
              setDirty(true);
            }}
            className="w-20 h-7 px-2 rounded bg-canvas text-[13px] text-fg ring-1 ring-line outline-none focus:ring-accent font-mono"
          />
          <span className="text-fg-faint">
            DSM의 rsync용 SSH 포트 — 일반 SSH 포트와 다릅니다
          </span>
        </label>
        {dirty && (
          <div className="flex justify-end">
            <Btn tone="accent" onClick={save}>
              저장
            </Btn>
          </div>
        )}
      </div>

      <Row
        label="앱을 열 때"
        hint="첫 화면이 뜬 뒤와 30분마다 1차 구역을 살핍니다. NAS가 꺼져 있으면 조용히 넘어갑니다. 받은 적 없는 사진만 셉니다."
      >
        <div className="flex gap-1">
          {(
            [
              ["off", "안 함"],
              ["notify", "새 사진 알림"],
              ["pull", "저절로 내려받기"],
            ] as const
          ).map(([v, label]) => (
            <button
              key={v}
              onClick={() => setNasAuto(v)}
              className={`h-6 px-2 rounded text-[13px] ${
                nasAuto === v
                  ? "bg-accent text-accent-fg"
                  : "text-fg-dim ring-1 ring-line hover:text-white"
              }`}
            >
              {label}
            </button>
          ))}
        </div>
      </Row>
      <Row
        label="1차 구역 내려받기"
        hint={
          desks.length === 0
            ? "작업대 라이브러리가 없습니다. 운영 SSD의 작업대 폴더를 «작업대» 역할로 등록하세요."
            : "폰·가족·구글포토가 먼저 닿는 1차 구역을 작업대의 NAS-1차/로. 거기서 고르고 정리합니다."
        }
      >
        <div className="flex gap-1">
          {desks.map((l) => (
            <Btn
              key={l.id}
              tone="accent"
              disabled={busy}
              onClick={() => pull(l.id)}
            >
              {desks.length > 1 ? `→ ${l.name}` : "내려받기"}
            </Btn>
          ))}
        </div>
      </Row>

      <Row
        label="올라갔나 확인"
        hint="내사진은 NAS 개인 폴더와, 공용은 NAS 공용 폴더와 견줍니다. 있는 것은 «NAS에 있음»으로 표시돼 찾기 칩으로 거를 수 있습니다."
      >
        <span />
      </Row>
      {mirrored.map((l) => {
        const v = verified[l.id];
        return (
          <div
            key={l.id}
            className="px-4 pb-2 flex items-center gap-3 text-[13px]"
          >
            <span className="w-28 shrink-0 text-fg truncate">
              <span className="text-fg-mute mr-1">{areaLabel(l.area)}</span>
              {l.name}
            </span>
            <span className="flex-1 text-fg-mute truncate">
              {v
                ? `NAS에 있음 ${v.present.toLocaleString()}장 · 없음 ${v.missing.toLocaleString()}장${v.sample.length ? ` — ${v.sample.slice(0, 3).join(", ")}${v.missing > 3 ? " …" : ""}` : ""}`
                : "아직 확인 안 함"}
            </span>
            <Btn
              disabled={busy || verifying !== null}
              onClick={() => verify(l.id)}
            >
              {verifying === l.id ? "확인 중…" : "NAS 확인"}
            </Btn>
          </div>
        );
      })}

      <Row
        label="1차 구역 비우기"
        hint={
          plan
            ? `옮겨도 되는 것 ${plan.items.length.toLocaleString()}개 · ${fmtBytes(plan.bytes)}. 아직 작업대에 있는 것 ${plan.pending.toLocaleString()}개, 행방을 모르는 것 ${plan.unknown.toLocaleString()}개는 둡니다.`
            : "우리가 내려받았고, 작업대에서 정리돼 NAS에 올라간 것이 확인됐거나 버린 것만 1차 구역의 #trash/로 옮깁니다. 지우진 않습니다."
        }
      >
        <div className="flex gap-1">
          {plan && plan.items.length > 0 ? (
            <Btn tone="drop" disabled={purging} onClick={runPurge}>
              {purging ? "옮기는 중…" : "#trash로"}
            </Btn>
          ) : (
            desks.map((l) => (
              <Btn key={l.id} disabled={busy} onClick={() => lookPurge(l.id)}>
                {desks.length > 1 ? `살펴보기 (${l.name})` : "살펴보기"}
              </Btn>
            ))
          )}
        </div>
      </Row>

      <Row
        label="XMP 사이드카"
        hint="평점·판정·즐겨찾기·태그를 파일 옆 .xmp에 적습니다 — nas_photo와 값을 맞추는 통로. 남이 만든 사이드카는 건드리지 않습니다."
      >
        <Btn
          disabled={busy}
          onClick={() =>
            invoke("xmp_export", { libraryId: null }).catch((e) =>
              toast(String(e), "drop"),
            )
          }
        >
          전부 내보내기
        </Btn>
      </Row>
    </Section>
  );
}
