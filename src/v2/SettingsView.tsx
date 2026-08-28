import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { useConfirm } from "./confirmContext";
import { useData } from "./dataStore";
import { fmtBytes, fmtDateTime } from "./format";
import { STYLES } from "./gridStyle";
import { useJob } from "./jobStore";
import { etaSec, fmtEta, rateOf } from "./rate";
import { DEFAULT_PREFS, usePref, usePrefs, type Prefs } from "./prefs";
import { toast } from "./toastStore";
import { Btn } from "./ui";
import { useUi } from "./uiStore";

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
};

function Ai() {
  const [st, setSt] = useState<AiStatus | null>(null);
  const job = useJob((s) => s.job);
  const busy = job !== null;
  const embedding = job?.label === "AI 벡터";
  // 최근 30초의 실제 속도 — 알림이 올 때마다 다시 센다
  const rate = useJob((s) =>
    s.job?.label === "AI 벡터" ? rateOf(s.samples, Date.now()) : null,
  );
  const reload = useCallback(() => {
    invoke<AiStatus>("ai_status")
      .then(setSt)
      .catch(() => setSt(null));
  }, []);
  // 일이 시작·끝날 때, 그리고 도는 동안엔 10초마다 다시 센다
  useEffect(() => {
    reload();
    if (!busy) return;
    const t = setInterval(reload, 10_000);
    return () => clearInterval(t);
  }, [reload, busy]);

  const download = async () => {
    try {
      await invoke("ai_model_download");
    } catch (e) {
      toast(String(e), "drop");
    }
  };
  const embed = async () => {
    try {
      await invoke("ai_embed_start");
    } catch (e) {
      toast(String(e), "drop");
    }
  };
  const stop = () => invoke("scan_cancel").catch(() => {});

  // 도는 중엔 상태바와 같은 숫자를 여기서도 — 끝날 때까지 «0장»으로 보이면 멎은 줄 안다
  const done = embedding
    ? (st?.embedded ?? 0) + (job?.done ?? 0)
    : (st?.embedded ?? 0);
  const total = st?.total ?? job?.total ?? 0;
  const left = Math.max(0, total - done);
  const hint = !st
    ? "…"
    : embedding
      ? `${done.toLocaleString()} / ${total.toLocaleString()}장 — 만드는 중. 남은 ${left.toLocaleString()}장${rate === null ? ", 속도 재는 중" : `, 초당 ${Math.round(rate)}장이면 ${fmtEta(etaSec(left, rate))}`}. 멈춰도 한 것은 남습니다.`
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
          <Btn tone="accent" disabled={busy} onClick={download}>
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
  const [ver, setVer] = useState("");
  useEffect(() => {
    getVersion()
      .then(setVer)
      .catch(() => setVer(""));
  }, []);
  return (
    <Section id="about" title="정보">
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
