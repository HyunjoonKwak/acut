import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import {
  Settings,
  Eye,
  Clock,
  Server,
  Sun,
  Moon,
  Monitor,
  Globe,
  Plus,
  Trash2,
  ToggleLeft,
  ToggleRight,
} from "lucide-react";
import type { AppConfig, ScheduleConfig, McpStatus } from "@/types";

type Tab = "general" | "watch" | "schedule" | "mcp";

export function SettingsView() {
  const { t, i18n } = useTranslation();

  const [activeTab, setActiveTab] = useState<Tab>("general");
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(true);

  // Schedule state
  const [schedules, setSchedules] = useState<ScheduleConfig[]>([]);
  const [newScheduleName, setNewScheduleName] = useState("");
  const [newScheduleCron, setNewScheduleCron] = useState("");
  const [newScheduleTaskType, setNewScheduleTaskType] = useState("sync");

  // MCP state
  const [mcpStatus, setMcpStatus] = useState<McpStatus | null>(null);

  useEffect(() => {
    const load = async () => {
      try {
        const cfg = await invoke<AppConfig>("get_config");
        setConfig(cfg);
      } catch {
        // ignore
      }
      setLoading(false);
    };
    load();
  }, []);

  useEffect(() => {
    if (activeTab === "schedule") {
      invoke<ScheduleConfig[]>("get_schedules")
        .then(setSchedules)
        .catch(() => {});
    }
    if (activeTab === "mcp") {
      invoke<McpStatus>("get_mcp_status")
        .then(setMcpStatus)
        .catch(() => {});
    }
  }, [activeTab]);

  const updateConfig = async (partial: Partial<AppConfig>) => {
    try {
      await invoke("update_config", { partial });
      setConfig((prev) => (prev ? { ...prev, ...partial } : prev));
    } catch {
      // ignore
    }
  };

  const handleResetConfig = async () => {
    try {
      await invoke("reset_config");
      const cfg = await invoke<AppConfig>("get_config");
      setConfig(cfg);
    } catch {
      // ignore
    }
  };

  // Theme
  const handleThemeChange = (theme: string) => {
    updateConfig({ theme });
  };

  // Language
  const handleLanguageChange = (language: string) => {
    updateConfig({ language });
    i18n.changeLanguage(language);
  };

  // Watch
  const handleWatchUpdate = (partial: Partial<AppConfig["watch"]>) => {
    if (!config) return;
    const watch = { ...config.watch, ...partial };
    updateConfig({ watch });
  };

  // Scan exclusions (line-based editors)
  const [excludeNamesDraft, setExcludeNamesDraft] = useState("");
  const [excludeSuffixesDraft, setExcludeSuffixesDraft] = useState("");
  useEffect(() => {
    if (!config?.scan) return;
    setExcludeNamesDraft(config.scan.exclude_dir_names.join("\n"));
    setExcludeSuffixesDraft(config.scan.exclude_suffixes.join("\n"));
  }, [config?.scan]);

  const handleSaveScanExclusions = () => {
    const parseLines = (text: string) =>
      text
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0);
    updateConfig({
      scan: {
        exclude_dir_names: parseLines(excludeNamesDraft),
        exclude_suffixes: parseLines(excludeSuffixesDraft),
      },
    });
  };

  // Schedule
  const handleAddSchedule = async () => {
    if (!newScheduleName.trim() || !newScheduleCron.trim()) return;
    const schedule: ScheduleConfig = {
      id: crypto.randomUUID(),
      name: newScheduleName.trim(),
      cron_expression: newScheduleCron.trim(),
      task_type: newScheduleTaskType,
      task_params_json: null,
      enabled: true,
    };
    try {
      await invoke("add_schedule", { schedule });
      setSchedules((prev) => [...prev, schedule]);
      setNewScheduleName("");
      setNewScheduleCron("");
    } catch {
      // ignore
    }
  };

  const handleRemoveSchedule = async (scheduleId: string) => {
    try {
      await invoke("remove_schedule", { scheduleId });
      setSchedules((prev) => prev.filter((s) => s.id !== scheduleId));
    } catch {
      // ignore
    }
  };

  const handleToggleSchedule = async (
    scheduleId: string,
    enabled: boolean
  ) => {
    try {
      await invoke("toggle_schedule", { scheduleId, enabled });
      setSchedules((prev) =>
        prev.map((s) => (s.id === scheduleId ? { ...s, enabled } : s))
      );
    } catch {
      // ignore
    }
  };

  // MCP
  const handleStartMcp = async () => {
    try {
      await invoke("start_mcp_server");
      const status = await invoke<McpStatus>("get_mcp_status");
      setMcpStatus(status);
      updateConfig({ mcp_enabled: true });
    } catch {
      // ignore
    }
  };

  const handleStopMcp = async () => {
    try {
      await invoke("stop_mcp_server");
      const status = await invoke<McpStatus>("get_mcp_status");
      setMcpStatus(status);
      updateConfig({ mcp_enabled: false });
    } catch {
      // ignore
    }
  };

  const tabs: { id: Tab; label: string; icon: typeof Settings }[] = [
    { id: "general", label: t("settings.general"), icon: Settings },
    { id: "watch", label: t("settings.watch"), icon: Eye },
    { id: "schedule", label: t("settings.schedule"), icon: Clock },
    { id: "mcp", label: t("settings.mcp"), icon: Server },
  ];

  if (loading || !config) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="w-5 h-5 border-2 border-accent border-t-transparent rounded-full animate-spin" />
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Header */}
      <div className="p-4 border-b border-border">
        <div className="flex items-center gap-3">
          <Settings size={20} className="text-accent" />
          <h2 className="text-sm font-semibold text-text-primary">
            {t("settings.title")}
          </h2>
        </div>
      </div>

      {/* Tabs */}
      <div className="flex border-b border-border px-4">
        {tabs.map((tab) => {
          const Icon = tab.icon;
          return (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center gap-2 px-4 py-2.5 text-xs font-medium border-b-2 transition-colors ${
                activeTab === tab.id
                  ? "border-accent text-accent"
                  : "border-transparent text-text-secondary hover:text-text-primary"
              }`}
            >
              <Icon size={14} />
              {tab.label}
            </button>
          );
        })}
      </div>

      {/* Tab content */}
      <div className="flex-1 overflow-y-auto p-4">
        {/* General tab */}
        {activeTab === "general" && (
          <div className="max-w-lg space-y-6">
            {/* Theme */}
            <div>
              <label className="text-xs font-medium text-text-primary block mb-2">
                {t("settings.theme")}
              </label>
              <div className="flex gap-2">
                {[
                  { value: "light", icon: Sun, label: t("settings.themeLight") },
                  { value: "dark", icon: Moon, label: t("settings.themeDark") },
                  {
                    value: "system",
                    icon: Monitor,
                    label: t("settings.themeSystem"),
                  },
                ].map(({ value, icon: Icon, label }) => (
                  <button
                    key={value}
                    onClick={() => handleThemeChange(value)}
                    className={`flex-1 flex items-center justify-center gap-2 px-3 py-2.5 rounded-md text-xs font-medium transition-colors ${
                      config.theme === value
                        ? "bg-accent text-white"
                        : "bg-bg-primary border border-border text-text-secondary hover:text-text-primary"
                    }`}
                  >
                    <Icon size={14} />
                    {label}
                  </button>
                ))}
              </div>
            </div>

            {/* Language */}
            <div>
              <label className="text-xs font-medium text-text-primary block mb-2">
                <Globe size={14} className="inline mr-1.5" />
                {t("settings.language")}
              </label>
              <div className="flex gap-2">
                {[
                  { value: "ko", label: "한국어" },
                  { value: "en", label: "English" },
                ].map(({ value, label }) => (
                  <button
                    key={value}
                    onClick={() => handleLanguageChange(value)}
                    className={`px-4 py-2.5 rounded-md text-xs font-medium transition-colors ${
                      config.language === value
                        ? "bg-accent text-white"
                        : "bg-bg-primary border border-border text-text-secondary hover:text-text-primary"
                    }`}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>

            {/* Scan exclusions */}
            <div className="pt-4 border-t border-border">
              <label className="text-xs font-medium text-text-primary block mb-1">
                {t("settings.scanExclude")}
              </label>
              <p className="text-[10px] text-text-secondary mb-2">
                {t("settings.scanExcludeDesc")}
              </p>
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <span className="text-[10px] text-text-secondary block mb-1">
                    {t("settings.scanExcludeNames")}
                  </span>
                  <textarea
                    value={excludeNamesDraft}
                    onChange={(e) => setExcludeNamesDraft(e.target.value)}
                    rows={6}
                    spellCheck={false}
                    className="w-full px-2 py-1.5 rounded-md text-[11px] font-mono bg-bg-primary border border-border text-text-primary focus:outline-none focus:border-accent/50 resize-y"
                  />
                </div>
                <div>
                  <span className="text-[10px] text-text-secondary block mb-1">
                    {t("settings.scanExcludeSuffixes")}
                  </span>
                  <textarea
                    value={excludeSuffixesDraft}
                    onChange={(e) => setExcludeSuffixesDraft(e.target.value)}
                    rows={6}
                    spellCheck={false}
                    className="w-full px-2 py-1.5 rounded-md text-[11px] font-mono bg-bg-primary border border-border text-text-primary focus:outline-none focus:border-accent/50 resize-y"
                  />
                </div>
              </div>
              <button
                onClick={handleSaveScanExclusions}
                className="mt-2 px-4 py-1.5 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover transition-colors"
              >
                {t("settings.scanExcludeSave")}
              </button>
            </div>

            {/* Reset */}
            <div className="pt-4 border-t border-border">
              <button
                onClick={handleResetConfig}
                className="px-4 py-2 rounded-md text-xs font-medium text-danger border border-danger/30 hover:bg-danger/10 transition-colors"
              >
                {t("settings.resetConfig")}
              </button>
            </div>
          </div>
        )}

        {/* Watch tab */}
        {activeTab === "watch" && (
          <div className="max-w-lg space-y-6">
            {/* Enable watch */}
            <div className="flex items-center justify-between">
              <div>
                <p className="text-xs font-medium text-text-primary">
                  {t("settings.watchEnabled")}
                </p>
                <p className="text-[10px] text-text-secondary mt-0.5">
                  {t("settings.watchEnabledDesc")}
                </p>
              </div>
              <button
                onClick={() =>
                  handleWatchUpdate({ enabled: !config.watch.enabled })
                }
                className="text-accent"
              >
                {config.watch.enabled ? (
                  <ToggleRight size={28} />
                ) : (
                  <ToggleLeft size={28} className="text-text-secondary" />
                )}
              </button>
            </div>

            {/* Debounce */}
            <div>
              <label className="text-xs font-medium text-text-primary block mb-1">
                {t("settings.debounceMs")}
              </label>
              <p className="text-[10px] text-text-secondary mb-2">
                {t("settings.debounceMsDesc")}
              </p>
              <input
                type="number"
                value={config.watch.debounce_ms}
                onChange={(e) =>
                  handleWatchUpdate({
                    debounce_ms: Math.max(0, parseInt(e.target.value) || 0),
                  })
                }
                min={0}
                step={100}
                className="w-32 px-3 py-2 rounded-md text-xs border border-border bg-bg-primary text-text-primary focus:outline-none focus:ring-1 focus:ring-accent"
              />
              <span className="text-[10px] text-text-secondary ml-2">ms</span>
            </div>

            {/* Auto-scan */}
            <div className="flex items-center justify-between">
              <div>
                <p className="text-xs font-medium text-text-primary">
                  {t("settings.autoScan")}
                </p>
                <p className="text-[10px] text-text-secondary mt-0.5">
                  {t("settings.autoScanDesc")}
                </p>
              </div>
              <button
                onClick={() =>
                  handleWatchUpdate({
                    auto_scan_on_change: !config.watch.auto_scan_on_change,
                  })
                }
                className="text-accent"
              >
                {config.watch.auto_scan_on_change ? (
                  <ToggleRight size={28} />
                ) : (
                  <ToggleLeft size={28} className="text-text-secondary" />
                )}
              </button>
            </div>
          </div>
        )}

        {/* Schedule tab */}
        {activeTab === "schedule" && (
          <div className="max-w-2xl space-y-4">
            {/* Add schedule */}
            <div className="flex items-end gap-2">
              <div className="flex-1">
                <label className="text-[10px] text-text-secondary block mb-1">
                  {t("settings.scheduleName")}
                </label>
                <input
                  type="text"
                  value={newScheduleName}
                  onChange={(e) => setNewScheduleName(e.target.value)}
                  placeholder={t("settings.scheduleNamePlaceholder")}
                  className="w-full px-3 py-2 rounded-md text-xs border border-border bg-bg-primary text-text-primary placeholder:text-text-secondary/50 focus:outline-none focus:ring-1 focus:ring-accent"
                />
              </div>
              <div className="w-40">
                <label className="text-[10px] text-text-secondary block mb-1">
                  {t("settings.cronExpression")}
                </label>
                <input
                  type="text"
                  value={newScheduleCron}
                  onChange={(e) => setNewScheduleCron(e.target.value)}
                  placeholder="0 0 * * *"
                  className="w-full px-3 py-2 rounded-md text-xs border border-border bg-bg-primary text-text-primary placeholder:text-text-secondary/50 focus:outline-none focus:ring-1 focus:ring-accent"
                />
              </div>
              <div className="w-28">
                <label className="text-[10px] text-text-secondary block mb-1">
                  {t("settings.taskType")}
                </label>
                <select
                  value={newScheduleTaskType}
                  onChange={(e) => setNewScheduleTaskType(e.target.value)}
                  className="w-full px-3 py-2 rounded-md text-xs border border-border bg-bg-primary text-text-primary focus:outline-none focus:ring-1 focus:ring-accent"
                >
                  <option value="sync">{t("settings.taskTypeSync")}</option>
                  <option value="scan">{t("settings.taskTypeScan")}</option>
                  <option value="organize">
                    {t("settings.taskTypeOrganize")}
                  </option>
                </select>
              </div>
              <button
                onClick={handleAddSchedule}
                disabled={!newScheduleName.trim() || !newScheduleCron.trim()}
                className="p-2 rounded-md bg-accent text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
                title={t("settings.addSchedule")}
              >
                <Plus size={14} />
              </button>
            </div>

            {/* Schedule list */}
            <div className="space-y-1">
              {schedules.length === 0 && (
                <p className="text-xs text-text-secondary text-center py-6">
                  {t("settings.noSchedules")}
                </p>
              )}
              {schedules.map((schedule) => (
                <div
                  key={schedule.id}
                  className="flex items-center gap-3 px-3 py-2.5 rounded-md border border-border bg-bg-primary group"
                >
                  <button
                    onClick={() =>
                      handleToggleSchedule(schedule.id, !schedule.enabled)
                    }
                    className="shrink-0"
                  >
                    {schedule.enabled ? (
                      <ToggleRight size={22} className="text-accent" />
                    ) : (
                      <ToggleLeft size={22} className="text-text-secondary" />
                    )}
                  </button>
                  <div className="flex-1 min-w-0">
                    <p
                      className={`text-xs font-medium ${schedule.enabled ? "text-text-primary" : "text-text-secondary"}`}
                    >
                      {schedule.name}
                    </p>
                    <p className="text-[10px] text-text-secondary">
                      {schedule.cron_expression} &middot; {schedule.task_type}
                    </p>
                  </div>
                  <button
                    onClick={() => handleRemoveSchedule(schedule.id)}
                    className="p-1 rounded opacity-0 group-hover:opacity-100 hover:text-danger transition-all"
                    title={t("settings.removeSchedule")}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* MCP tab */}
        {activeTab === "mcp" && (
          <div className="max-w-lg space-y-6">
            {/* Enable MCP */}
            <div className="flex items-center justify-between">
              <div>
                <p className="text-xs font-medium text-text-primary">
                  {t("settings.mcpEnabled")}
                </p>
                <p className="text-[10px] text-text-secondary mt-0.5">
                  {t("settings.mcpEnabledDesc")}
                </p>
              </div>
              <button
                onClick={
                  mcpStatus?.running ? handleStopMcp : handleStartMcp
                }
                className="text-accent"
              >
                {mcpStatus?.running ? (
                  <ToggleRight size={28} />
                ) : (
                  <ToggleLeft size={28} className="text-text-secondary" />
                )}
              </button>
            </div>

            {/* Status */}
            <div className="rounded-lg border border-border p-4">
              <p className="text-xs font-medium text-text-primary mb-2">
                {t("settings.mcpStatus")}
              </p>
              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <span className="text-[10px] text-text-secondary">
                    {t("settings.mcpRunning")}
                  </span>
                  <span
                    className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-medium ${
                      mcpStatus?.running
                        ? "bg-success/10 text-success"
                        : "bg-bg-secondary text-text-secondary"
                    }`}
                  >
                    <span
                      className={`w-1.5 h-1.5 rounded-full ${mcpStatus?.running ? "bg-success" : "bg-text-secondary"}`}
                    />
                    {mcpStatus?.running
                      ? t("settings.mcpActive")
                      : t("settings.mcpInactive")}
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-[10px] text-text-secondary">
                    {t("settings.mcpSocketPath")}
                  </span>
                  <span className="text-[10px] text-text-primary font-mono">
                    {mcpStatus?.socket_path || "-"}
                  </span>
                </div>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
