import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { useConfirm } from "./confirmContext";
import { useData } from "./dataStore";
import { fmtBytes, fmtDateTime } from "./format";
import { STYLES } from "./gridStyle";
import { useJob } from "./jobStore";
import { etaSec, fmtEta, pushSample, rateOf, type Sample } from "./rate";
import { DEFAULT_PREFS, usePref, usePrefs, type Prefs } from "./prefs";
import { toast } from "./toastStore";
import { Btn } from "./ui";
import { useUi } from "./uiStore";
import { areaLabel } from "./areaItems";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

type Backup = { path: string; name: string; bytes: number; made_at: number };

/**
 * 설정 — 본문을 통째로 쓴다 (Lap의 Settings.vue).
 *
 * 사이드바의 좁은 패널에 늘어놓다 보니 스크롤이 길어졌다. 왼쪽 목록에서
 * 갈래를 누르면 그 자리로 간다. 값은 전부 prefs에 — 켰다 꺼도 남는다.
 */
export default function SettingsView({
  onRescanAll,
}: {
  onRescanAll: () => void;
}) {
  return (
    <div className="flex-1 min-w-0 overflow-y-auto">
      <div className="max-w-[720px] mx-auto px-6 py-6 space-y-10">
        <General />
        <Library onRescanAll={onRescanAll} />
        <Browse />
        <ViewerSection />
        <Ai />
        <Database />
        <Backup />
        <Nas />
        <Advanced />
        <About />
      </div>
    </div>
  );
}

// ── 조각들 ──────────────────────────────────────────────────────────────

function Section({
  id,
  title,
  children,
}: {
  id: string;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section id={`settings-${id}`} className="scroll-mt-4">
      <h2 className="text-[10.5px] font-bold uppercase tracking-widest text-fg-mute mb-3">
        {title}
      </h2>
      <div className="rounded-lg bg-chrome ring-1 ring-line divide-y divide-line">
        {children}
      </div>
    </section>
  );
}

/** 한 줄 — 왼쪽에 이름과 설명, 오른쪽에 조작 */
function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-4 px-4 py-2.5">
      <div className="flex-1 min-w-0">
        <div className="text-[13px] text-fg">{label}</div>
        {hint && (
          <div className="text-[11.5px] text-fg-mute leading-snug mt-0.5">
            {hint}
          </div>
        )}
      </div>
      <div className="shrink-0 flex items-center gap-2">{children}</div>
    </div>
  );
}

function Select<K extends keyof Prefs>({
  k,
  options,
}: {
  k: K;
  options: { v: Prefs[K]; label: string }[];
}) {
  const [value, set] = usePref(k);
  return (
    <select
      value={String(value)}
      onChange={(e) => {
        const o = options.find((x) => String(x.v) === e.target.value);
        if (o) set(o.v);
      }}
      aria-label={String(k)}
      className="h-control min-w-[140px] px-2 rounded-md bg-raised text-[12.5px] text-fg ring-1 ring-line outline-none focus:ring-accent"
    >
      {options.map((o) => (
        <option key={String(o.v)} value={String(o.v)}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

function Toggle({ k }: { k: keyof Prefs }) {
  const [value, set] = usePref(k);
  const on = Boolean(value);
  return (
    <button
      role="switch"
      aria-checked={on}
      aria-label={String(k)}
      onClick={() => (set as (v: boolean) => void)(!on)}
      className={`relative w-9 h-5 rounded-full transition-colors ${on ? "bg-accent" : "bg-line-strong"}`}
    >
      <span
        className={`absolute left-0 top-0.5 w-4 h-4 rounded-full bg-white transition-transform ${
          on ? "translate-x-[18px]" : "translate-x-0.5"
        }`}
      />
    </button>
  );
}

// ── 갈래들 ──────────────────────────────────────────────────────────────

function General() {
  return (
    <Section id="general" title="일반">
      <Row
        label="글꼴"
        hint="프리텐다드는 앱에 들어 있어 어디서나 같게 보입니다. 시스템은 SF Pro + Apple SD Gothic Neo."
      >
        <Select
          k="font"
          options={[
            { v: "pretendard", label: "프리텐다드" },
            { v: "system", label: "시스템 글꼴" },
          ]}
        />
      </Row>
      <Row
        label="테마"
        hint="어두운 테마 하나입니다. 밝은 테마는 아직 없습니다."
      >
        <span className="text-[12.5px] text-fg-mute">어두움</span>
      </Row>
      <Row label="이름표" hint="레일·툴바 버튼에 커서를 올리면 뜨는 설명">
        <Toggle k="tooltips" />
      </Row>
      <Row
        label="상태바"
        hint="아래쪽 한 줄 — 지금 보고 있는 사진의 정보와 진행"
      >
        <Toggle k="statusBar" />
      </Row>
    </Section>
  );
}

function Library({ onRescanAll }: { onRescanAll: () => void }) {
  const scanMsg = useData((s) => s.scanMsg);
  const scanning = useJob((s) => s.job !== null);
  return (
    <Section id="library" title="라이브러리">
      <Row
        label="폴더 감시"
        hint="파인더로 넣거나 지운 사진이 저절로 반영됩니다. 복사가 멎고 1.5초 뒤에 그 폴더만 다시 봅니다."
      >
        <Toggle k="watch" />
      </Row>
      <Row
        label="다시 스캔"
        hint={
          scanMsg ||
          "새로 들어온 사진과 썸네일을 채웁니다. 이미 아는 것은 건너뜁니다."
        }
      >
        <Btn disabled={scanning} onClick={onRescanAll}>
          {scanning ? "스캔 중…" : "전부 다시 스캔"}
        </Btn>
      </Row>
    </Section>
  );
}

function Browse() {
  const [thumbSize, setThumbSize] = usePref("thumbSize");
  const [style] = usePref("gridStyle");
  return (
    <Section id="browse" title="탐색">
      <Row label="보기 방식" hint="툴바 버튼과 같은 것입니다">
        <Select
          k="gridStyle"
          options={STYLES.map((s) => ({ v: s.v, label: s.label }))}
        />
      </Row>
      <Row label="썸네일 크기">
        <input
          type="range"
          min={100}
          max={320}
          value={thumbSize}
          onChange={(e) => setThumbSize(+e.target.value)}
          aria-label="썸네일 크기"
          className="w-40 accent-accent"
        />
        <span className="w-10 text-right text-[12px] text-fg-mute tabular-nums">
          {thumbSize}
        </span>
      </Row>
      <Row
        label="이름줄 첫째 줄"
        hint={style === "card" ? undefined : "카드 보기에서만 뜹니다"}
      >
        <Select
          k="caption1"
          options={[
            { v: "name", label: "파일 이름" },
            { v: "date", label: "촬영일" },
          ]}
        />
      </Row>
      <Row label="이름줄 둘째 줄">
        <Select
          k="caption2"
          options={[
            { v: "dateSize", label: "촬영일 · 크기" },
            { v: "date", label: "촬영일" },
            { v: "size", label: "크기" },
            { v: "camera", label: "카메라" },
            { v: "none", label: "없음" },
          ]}
        />
      </Row>
      <Row label="타일 배지" hint="타일 왼쪽 아래에 붙는 작은 표시">
        <Select
          k="badge"
          options={[
            { v: "none", label: "없음" },
            { v: "format", label: "형식 (RAW·영상)" },
            { v: "iso", label: "ISO" },
            { v: "shutter", label: "셔터 속도" },
            { v: "aperture", label: "조리개" },
            { v: "focal", label: "초점 거리" },
          ]}
        />
      </Row>
      <Row label="두 번 누르면">
        <Select
          k="dblClick"
          options={[
            { v: "viewer", label: "크게 보기" },
            { v: "app", label: "기본 앱으로 열기" },
          ]}
        />
      </Row>
      <Row label="필름스트립 띠 자리">
        <Select
          k="stripPos"
          options={[
            { v: "top", label: "위" },
            { v: "bottom", label: "아래" },
          ]}
        />
      </Row>
    </Section>
  );
}

function ViewerSection() {
  return (
    <Section id="viewer" title="뷰어">
      <Row label="마우스 휠" hint="확대는 커서 자리를 기준으로 합니다">
        <Select
          k="wheel"
          options={[
            { v: "zoom", label: "확대·축소" },
            { v: "next", label: "앞뒤 사진" },
          ]}
        />
      </Row>
      <Row label="배경">
        <Select
          k="viewerBg"
          options={[
            { v: "canvas", label: "앱 바탕" },
            { v: "black", label: "검정" },
            { v: "gray", label: "회색" },
          ]}
        />
      </Row>
      <Row label="슬라이드쇼 간격">
        <Select
          k="slideshowSec"
          options={[2, 3, 5, 10].map((n) => ({ v: n, label: `${n}초` }))}
        />
      </Row>
      <Row label="영상 자동 재생" hint="크게 보기와 필름스트립에서">
        <Toggle k="autoplay" />
      </Row>
      <Row label="영상 반복">
        <Toggle k="loopVideo" />
      </Row>
    </Section>
  );
}

type AiStatus = {
  model_present: boolean;
  model_bytes: number;
  embedded: number;
  total: number;
  running: boolean;
  text_present: boolean;
  text_bytes: number;
  face_present: boolean;
  face_bytes: number;
  faces_done: number;
  faces_total: number;
  faces: number;
  persons: number;
};

function Ai() {
  const [st, setSt] = useState<AiStatus | null>(null);
  // 개수 표본 — DB가 진실이다. 이벤트는 화면이 새로 뜨면 놓치지만 개수는 안 놓친다.
  const [samples, setSamples] = useState<Sample[]>([]);
  const job = useJob((s) => s.job);
  const busy = job !== null;
  // 누른 직후 — 개수가 움직이기 전 몇 초 동안도 «만드는 중»으로
  const [kicked, setKicked] = useState(0);
  // 마지막으로 센 시각 — 그리는 동안 시계를 읽지 않는다
  const [now, setNow] = useState(0);
  const reload = useCallback(() => {
    invoke<AiStatus>("ai_status")
      .then((s) => {
        const t = Date.now();
        setSt(s);
        setNow(t);
        setSamples((prev) => pushSample(prev, { t, n: s.embedded }));
      })
      .catch(() => setSt(null));
  }, []);
  // 3초마다 다시 센다 — 개수가 오르면 도는 것이고, 안 오르면 멎은 것이다
  useEffect(() => {
    reload();
    const t = setInterval(reload, 3_000);
    return () => clearInterval(t);
  }, [reload]);

  const download = async (which: "vision" | "text" | "face") => {
    try {
      await invoke("ai_model_download", { which });
    } catch (e) {
      toast(String(e), "drop");
    }
  };
  const embed = async () => {
    try {
      await invoke("ai_embed_start");
      setKicked(Date.now());
    } catch (e) {
      toast(String(e), "drop");
    }
  };
  const stop = () => invoke("scan_cancel").catch(() => {});

  const rate = rateOf(samples, now);
  const growing = rate !== null && rate > 0;
  // 뒷단이 «도는 중»이라 하면 그게 진실. 개수가 오르는 것과 방금 누른 것은 거들 뿐.
  const embedding =
    (st?.running ?? false) ||
    growing ||
    job?.label === "AI 벡터" ||
    now - kicked < 15_000;
  const done = st?.embedded ?? 0;
  const total = st?.total ?? 0;
  const left = Math.max(0, total - done);
  const hint = !st
    ? "…"
    : embedding
      ? `${done.toLocaleString()} / ${total.toLocaleString()}장 — 만드는 중. 남은 ${left.toLocaleString()}장${growing ? `, 초당 ${Math.round(rate)}장이면 ${fmtEta(etaSec(left, rate))}` : ", 속도 재는 중"}. 멈춰도 한 것은 남습니다.`
      : left > 0
        ? `${done.toLocaleString()} / ${total.toLocaleString()}장 — 남은 ${left.toLocaleString()}장. 하다 말아도 한 것은 남습니다.`
        : `${done.toLocaleString()}장 전부 있습니다. 새로 들어온 사진만 더 만들면 됩니다.`;

  return (
    <Section id="ai" title="AI">
      <Row
        label="모델"
        hint={
          st?.model_present
            ? "CLIP ViT-B/32 — 사진을 512개 숫자로 요약합니다. 전부 이 맥 안에서 돕니다."
            : `CLIP ViT-B/32 (${st ? fmtBytes(st.model_bytes) : "…"}) — 한 번만 받습니다. 그 뒤로는 네트워크가 필요 없습니다.`
        }
      >
        {st?.model_present ? (
          <span className="text-[12.5px] text-keep">받아 둠</span>
        ) : (
          <Btn tone="accent" disabled={busy} onClick={() => download("vision")}>
            받기
          </Btn>
        )}
      </Row>
      <Row
        label="글로 찾기 모델"
        hint={
          st?.text_present
            ? "다국어 텍스트 모델 — «바닷가 강아지»처럼 글로 찾습니다. 찾기 갈래의 «AI로 찾기»에 씁니다."
            : `다국어 텍스트 모델 (${st ? fmtBytes(st.text_bytes) : "…"}) — 한국어·영어로 사진을 찾습니다. 사진 벡터가 있어야 씁니다.`
        }
      >
        {st?.text_present ? (
          <span className="text-[12.5px] text-keep">받아 둠</span>
        ) : (
          <Btn tone="accent" disabled={busy} onClick={() => download("text")}>
            받기
          </Btn>
        )}
      </Row>
      <Row label="사진 벡터" hint={hint}>
        {embedding ? (
          <Btn tone="drop" onClick={stop}>
            멈추기
          </Btn>
        ) : (
          <Btn
            tone="accent"
            disabled={busy || !st?.model_present || left === 0}
            onClick={embed}
          >
            벡터 만들기
          </Btn>
        )}
      </Row>
      {embedding && total > 0 && (
        <div className="px-4 pb-3">
          <div className="h-1 rounded-full bg-raised overflow-hidden">
            <div
              className="h-full bg-accent transition-[width] duration-300"
              style={{ width: `${Math.min(100, (done / total) * 100)}%` }}
            />
          </div>
        </div>
      )}
      <Row
        label="얼굴 모델"
        hint={
          st?.face_present
            ? "YuNet(찾기)·SFace(알아보기) — OpenCV zoo, Apache-2.0. 전부 이 맥 안에서 돕니다."
            : `YuNet·SFace (${st ? fmtBytes(st.face_bytes) : "…"}) — 얼굴을 찾아 사람으로 묶습니다.`
        }
      >
        {st?.face_present ? (
          <span className="text-[12.5px] text-keep">받아 둠</span>
        ) : (
          <Btn tone="accent" disabled={busy} onClick={() => download("face")}>
            받기
          </Btn>
        )}
      </Row>
      <Row
        label="얼굴 찾기"
        hint={
          !st
            ? "…"
            : st.faces_total === 0
              ? "썸네일이 있어야 찾습니다."
              : `${st.faces_done.toLocaleString()} / ${st.faces_total.toLocaleString()}장에서 얼굴 ${st.faces.toLocaleString()}개, ${st.persons.toLocaleString()}명. 왼쪽 「사람」 갈래에서 이름을 붙이고 합칩니다.`
        }
      >
        <Btn
          tone="accent"
          disabled={
            busy ||
            !st?.face_present ||
            (st?.faces_total ?? 0) - (st?.faces_done ?? 0) === 0
          }
          onClick={() =>
            invoke("ai_faces_start").catch((e) => toast(String(e), "drop"))
          }
        >
          얼굴 찾기
        </Btn>
      </Row>
      <Row
        label="비슷한 사진 찾기"
        hint="사진을 우클릭해 「비슷한 사진 찾기」. 벡터가 있는 사진끼리 비교합니다."
      >
        <span />
      </Row>
    </Section>
  );
}

function Database() {
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
 * NAS — 종(從)이다. 동기화는 Drive Client가, 에이컷은 1차 구역을 내려받고,
 * 올라갔는지 확인하고, 확인된 것만 1차에서 비운다. 순서가 곧 안전장치.
 */
function Nas() {
  const ask = useConfirm();
  const libs = useData((s) => s.libs);
  const job = useJob((s) => s.job);
  const busy = job !== null;
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
    <label key={k} className="flex items-center gap-2 text-[12px]">
      <span className="w-20 shrink-0 text-fg-mute">{label}</span>
      <input
        value={cfg?.[k] ?? ""}
        onChange={(e) => {
          setCfg((c) => (c ? { ...c, [k]: e.target.value } : c));
          setDirty(true);
        }}
        className="flex-1 min-w-0 h-7 px-2 rounded bg-canvas text-[12px] text-fg ring-1 ring-line outline-none focus:ring-accent font-mono"
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
        <div className="mx-4 mb-3 px-3 py-2 rounded-md bg-raised text-[12px] flex items-start gap-2">
          <span
            className="mt-1.5 w-2 h-2 rounded-full shrink-0"
            style={{
              background:
                st.online && st.rsync_ok
                  ? "var(--color-keep)"
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
        <label className="flex items-center gap-2 text-[12px]">
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
            className="w-20 h-7 px-2 rounded bg-canvas text-[12px] text-fg ring-1 ring-line outline-none focus:ring-accent font-mono"
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
            className="px-4 pb-2 flex items-center gap-3 text-[12px]"
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

function Backup() {
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
        <div className="px-4 pb-3 text-[11.5px] text-fg-mute tabular-nums">
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

function Advanced() {
  const ask = useConfirm();
  const setUi = useUi((s) => s.set);
  const reset = async () => {
    const ok = await ask({
      title: "설정을 처음 상태로 되돌립니다",
      lines: ["· 보기·크기·글꼴 같은 설정만 — 사진·판정·태그는 그대로입니다"],
      confirmLabel: "되돌리기",
    });
    if (!ok) return;
    const set = usePrefs.getState().set;
    (Object.keys(DEFAULT_PREFS) as (keyof Prefs)[]).forEach((k) =>
      (set as (k: keyof Prefs, v: unknown) => void)(k, DEFAULT_PREFS[k]),
    );
    toast("설정을 되돌렸습니다");
  };
  return (
    <Section id="advanced" title="고급">
      <Row label="단축키" hint="목록·크게 보기·나란히 보기의 키를 한 장에">
        <Btn hint="?" onClick={() => setUi({ helping: true })}>
          보기
        </Btn>
      </Row>
      <Row label="설정 초기화">
        <Btn onClick={reset}>처음 상태로</Btn>
      </Row>
    </Section>
  );
}

function About() {
  const [startup, setStartup] = useState<{
    db_ms: number;
    first_grid_ms: number;
    at: number;
    marks?: Record<string, number>;
  } | null>(null);
  const [ver, setVer] = useState("");
  useEffect(() => {
    invoke<string | null>("settings_get", { key: "startup.last" })
      .then((v) => setStartup(v ? JSON.parse(v) : null))
      .catch(() => {});
    getVersion()
      .then(setVer)
      .catch(() => setVer(""));
  }, []);
  return (
    <Section id="about" title="정보">
      <Row
        label="마지막 시작"
        hint={
          startup
            ? `프로세스 시작 → DB 준비 ${startup.db_ms}ms · 첫 그리드 ${startup.first_grid_ms}ms (${fmtDateTime(startup.at)}). 목표는 1초.${
                startup.marks
                  ? ` 웹뷰 기준: ${Object.entries(startup.marks)
                      .map(([k, v]) => `${k} ${v}`)
                      .join(" · ")}`
                  : ""
              }`
            : "아직 잰 적 없습니다."
        }
      >
        <span />
      </Row>
      <Row
        label="에이컷"
        hint="대규모 로컬 라이브러리를 위한 오프라인 우선 사진 관리자. 사진은 원래 자리에 그대로 둡니다."
      >
        <span className="text-[12.5px] text-fg-mute tabular-nums">
          {ver ? `v${ver}` : "—"}
        </span>
      </Row>
      <Row
        label="글꼴"
        hint="Pretendard — Kil Hyung-jin, SIL Open Font License 1.1"
      >
        <span />
      </Row>
    </Section>
  );
}
