import { useEffect, useMemo, useState } from "react"
import MarkdownRenderer from "@/components/common/MarkdownRenderer"
import { useTranslation } from "react-i18next"
import { skillSourceLabel } from "./skillSourceLabel"
import { cn } from "@/lib/utils"
import { formatBytes } from "@/lib/format"
import { IconTip } from "@/components/ui/tooltip"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  ArrowLeft,
  Check,
  Download,
  ExternalLink,
  File,
  Folder,
  FolderOpen,
  Trash2,
} from "lucide-react"
import type { SkillInstallSpec } from "../types"
import type {
  AppKind,
  SkillAppInstallState,
  SkillDetail,
  SkillDockSnapshot,
  SkillStatusEntry,
} from "./types"
import {
  exportSkillZip,
  installSkillDependency,
  installSkillToApp,
  uninstallSkillFromApp,
} from "./api"

const DETAIL_APPS = ["claude", "codex", "gemini", "opencode"] as const
type DetailAppKind = Extract<AppKind, (typeof DETAIL_APPS)[number]>
type DetailTab = "overview" | "skill" | "validation" | "install" | "diff"

function InstallSpecRow({
  spec,
  skillName,
  specIndex,
}: {
  spec: SkillInstallSpec
  skillName: string
  specIndex: number
}) {
  const { t } = useTranslation()
  const [installing, setInstalling] = useState(false)
  const [result, setResult] = useState<{ ok: boolean; message: string } | null>(null)
  const label =
    spec.label || `${spec.kind}: ${spec.formula || spec.package || spec.go_module || "?"}`

  async function handleInstall() {
    setInstalling(true)
    setResult(null)
    try {
      const output = await installSkillDependency(skillName, specIndex)
      setResult({ ok: true, message: output })
    } catch (e) {
      setResult({ ok: false, message: String(e) })
    } finally {
      setInstalling(false)
    }
  }

  return (
    <div className="flex items-center gap-2 rounded-md border border-[#f0f0f0] bg-[#fafafa] px-3 py-2">
      <span className="rounded bg-[#f3f4f6] px-1.5 py-0.5 font-mono text-[10px] text-[#6b7280]">
        {spec.kind}
      </span>
      <span className="min-w-0 flex-1 truncate text-xs text-[#374151]">{label}</span>
      <Button
        variant="secondary"
        size="sm"
        className="h-7 border-[#e5e7eb] bg-white px-2 text-[11px] text-[#374151]"
        onClick={handleInstall}
        disabled={installing}
      >
        {installing
          ? t("settings.skillInstalling")
          : result?.ok
            ? t("settings.skillInstallSuccess")
            : result && !result.ok
              ? t("settings.skillInstallFailed")
              : t("settings.skillInstall")}
      </Button>
    </div>
  )
}

interface SkillDetailViewProps {
  skill: SkillDetail
  envStatus: Record<string, Record<string, boolean>>
  status?: SkillStatusEntry
  dockSnapshot?: SkillDockSnapshot | null
  envValues: Record<string, string>
  envDirty: Record<string, boolean>
  envSaving: Record<string, boolean>
  onBack: () => void
  onToggleSkill: (name: string, enabled: boolean) => void
  onOpenDir: (path: string) => void
  onEnvValueChange: (key: string, value: string) => void
  onSaveEnvVar: (key: string) => void
  onRemoveEnvVar: (key: string) => void
}

export default function SkillDetailView({
  skill,
  envStatus,
  status,
  dockSnapshot,
  envValues,
  envDirty,
  envSaving,
  onBack,
  onToggleSkill,
  onOpenDir,
  onEnvValueChange,
  onSaveEnvVar,
  onRemoveEnvVar,
}: SkillDetailViewProps) {
  const { t } = useTranslation()
  const [activeTab, setActiveTab] = useState<DetailTab>("skill")
  const [actionBusy, setActionBusy] = useState<string | null>(null)
  const [selectedApp, setSelectedApp] = useState<DetailAppKind>("claude")
  const [exportPath, setExportPath] = useState(`${skill.name}.zip`)
  const requiresEnv = skill.requires?.env ?? []
  const missingBins = status?.missing_bins ?? []
  const missingAnyBins = status?.missing_any_bins ?? []
  const missingEnv = status?.missing_env ?? []
  const missingConfig = status?.missing_config ?? []
  const hardBlocked = !!status?.hard_blocked
  const needsSetup = !!status?.needs_setup && !hardBlocked
  const hasWarning =
    hardBlocked ||
    needsSetup ||
    missingBins.length > 0 ||
    missingAnyBins.length > 0 ||
    missingEnv.length > 0 ||
    missingConfig.length > 0
  const usage = dockSnapshot?.usage?.find((row) => row.skillName === skill.name)
  const installStates = useMemo(
    () =>
      DETAIL_APPS.map(
        (app) =>
          usage?.apps.find((state) => state.app === app) ?? defaultInstallState(app, skill.name),
      ),
    [skill.name, usage?.apps],
  )
  const installedApps = installStates.filter((state) => state.installed)
  const defaultApp = installedApps[0]?.app ?? "claude"
  useEffect(() => {
    setSelectedApp(defaultApp)
  }, [defaultApp])
  const tags = skill.display?.tags?.length
    ? skill.display.tags
    : [skillSourceLabel(t, skill.source), ...(skill.paths ?? []).slice(0, 3)]
  const validationRows = [
    {
      label: t("settings.skillFiles"),
      ok: skill.files.length > 0,
      detail: `${skill.files.length}`,
    },
    {
      label: "YAML front matter",
      ok: !hardBlocked,
      detail: hardBlocked ? t("settings.skillHardBlocked") : t("settings.skillStatusEligible"),
    },
    {
      label: t("settings.skillSchemaCheck"),
      ok: !needsSetup,
      detail: needsSetup ? t("settings.skillNeedsSetup") : t("settings.skillStatusEligible"),
    },
    {
      label: t("settings.skillReferenceCheck"),
      ok: !hasWarning,
      warning: hasWarning,
      detail: hasWarning ? "1" : t("settings.skillStatusEligible"),
    },
  ]

  async function runDetailAction(label: string, fn: () => Promise<string | void>) {
    setActionBusy(label)
    try {
      const message = await fn()
      if (message) window.alert(message)
    } catch (error) {
      window.alert(error instanceof Error ? error.message : String(error))
    } finally {
      setActionBusy(null)
    }
  }

  async function handleInstallToApp() {
    const app = selectedApp
    await runDetailAction("install", async () => {
      await installSkillToApp(skill.name, app)
      return t("settings.skillInstallToAppResult", { name: skill.name, app })
    })
  }

  async function handleUninstallFromApp() {
    const app = selectedApp
    await runDetailAction("uninstall", async () => {
      await uninstallSkillFromApp(skill.name, app)
      return t("settings.skillUninstallFromAppResult", { name: skill.name, app })
    })
  }

  async function handleExportZip() {
    if (!exportPath.trim()) return
    await runDetailAction("export", async () => {
      const report = await exportSkillZip(skill.name, exportPath.trim())
      return t("settings.skillExportResult", { name: skill.name, path: report.outputPath })
    })
  }

  async function handleMoreAction(action: string) {
    if (action === "copyName") {
      await navigator.clipboard.writeText(skill.name)
      return
    }
    if (action === "copyPath") {
      await navigator.clipboard.writeText(skill.base_dir)
      return
    }
    if (action === "toggleEnabled") {
      await runDetailAction("toggle", async () => {
        await onToggleSkill(skill.name, !skill.enabled)
      })
    }
  }

  return (
    <div className="flex-1 min-h-0 overflow-y-auto bg-[#f5f5f7] p-5 text-[#1d1d1f] dark:bg-background dark:text-foreground">
      <div className="space-y-4">
        <header className="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
          <div className="min-w-0">
            <button
              type="button"
              className="mb-3 inline-flex items-center gap-1 text-xs text-[#888] hover:text-[#1d1d1f]"
              onClick={onBack}
            >
              <ArrowLeft className="h-3.5 w-3.5" />
              {t("settings.skills")}
            </button>
            <div className="flex items-center gap-3">
              <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-[10px] bg-[#dcfce7] text-xl font-semibold text-[#22c55e]">
                {skill.display?.emoji || "</>"}
              </div>
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <h2 className="truncate text-xl font-semibold text-[#1d1d1f]">{skill.name}</h2>
                  <button
                    type="button"
                    className={cn(
                      "rounded px-2 py-0.5 text-[11px] font-medium",
                      hasWarning ? "bg-[#fef3c7] text-[#92400e]" : "bg-[#dcfce7] text-[#166534]",
                    )}
                    onClick={() => onToggleSkill(skill.name, !skill.enabled)}
                  >
                    {hasWarning
                      ? t("settings.skillsManager.stateAttention")
                      : skill.enabled
                        ? t("settings.skillStatusEligible")
                        : t("settings.skillsManager.stateDisabled")}
                  </button>
                </div>
                <p className="mt-1 line-clamp-2 text-[13px] text-[#888]">{skill.description}</p>
              </div>
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              {tags.slice(0, 8).map((tag) => (
                <span key={tag} className="rounded bg-[#f3f4f6] px-2.5 py-1 text-xs text-[#6b7280]">
                  #{tag}
                </span>
              ))}
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Select value={selectedApp} onValueChange={(value) => setSelectedApp(value as DetailAppKind)}>
              <SelectTrigger className="h-9 w-[140px] rounded-md border-[#e5e7eb] bg-white text-[13px] text-[#374151]">
                <SelectValue placeholder={t("settings.skillSelectApp")} />
              </SelectTrigger>
              <SelectContent>
                {DETAIL_APPS.map((app) => (
                  <SelectItem key={app} value={app}>
                    {t(`settings.skillsDockExtensions.app.${app}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button
              className="h-9 gap-1.5 bg-[#2563eb] px-4 text-[13px] text-white"
              disabled={actionBusy !== null}
              onClick={() => void handleInstallToApp()}
            >
              <Download className="h-3.5 w-3.5" />
              {t("settings.skillInstallToApp")}
            </Button>
            <Button
              variant="secondary"
              className="h-9 gap-1.5 border-[#e5e7eb] bg-white px-3.5 text-[13px] text-[#374151]"
              disabled={actionBusy !== null}
              onClick={() => void handleUninstallFromApp()}
            >
              <Trash2 className="h-3.5 w-3.5" />
              {t("settings.skillUninstallFromApp")}
            </Button>
            <Button
              variant="secondary"
              className="h-9 gap-1.5 border-[#e5e7eb] bg-white px-3.5 text-[13px] text-[#374151]"
              onClick={() => onOpenDir(skill.base_dir)}
            >
              <FolderOpen className="h-3.5 w-3.5" />
              {t("settings.skillOpenFolder")}
            </Button>
            <Button
              variant="secondary"
              className="h-9 gap-1.5 border-[#e5e7eb] bg-white px-3.5 text-[13px] text-[#374151]"
              disabled={actionBusy !== null}
              onClick={() => void handleExportZip()}
            >
              <Download className="h-3.5 w-3.5" />
              {t("settings.skillExportZip")}
            </Button>
            <Input
              value={exportPath}
              onChange={(event) => setExportPath(event.target.value)}
              placeholder={t("settings.skillExportPathPlaceholder")}
              className="h-9 w-[220px] rounded-md border-[#e5e7eb] bg-white text-[13px]"
            />
            <Select onValueChange={(value) => void handleMoreAction(value)}>
              <SelectTrigger className="h-9 w-[140px] border-[#e5e7eb] bg-white px-3.5 text-[13px] text-[#374151]">
                <SelectValue placeholder={t("settings.skillMoreActions")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="copyName">{t("settings.skillCopyName")}</SelectItem>
                <SelectItem value="copyPath">{t("settings.skillCopyPath")}</SelectItem>
                <SelectItem value="toggleEnabled">
                  {skill.enabled ? t("settings.skillDisable") : t("settings.skillEnable")}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
        </header>

        <nav className="border-b border-[#e5e7eb]">
          {[
            ["overview", t("settings.skillOverviewTab")],
            ["skill", "SKILL.md"],
            ["validation", t("settings.skillValidationTab")],
            ["install", t("settings.skillInstallTab")],
            ["diff", t("settings.skillDiffTab")],
          ].map(([id, label]) => (
            <button
              key={id}
              type="button"
              className={cn(
                "px-4 py-2.5 text-[13px]",
                activeTab === id
                  ? "border-b-2 border-[#2563eb] text-[#2563eb]"
                  : "border-b-2 border-transparent text-[#888] hover:text-[#1d1d1f]",
              )}
              onClick={() => setActiveTab(id as DetailTab)}
            >
              {label}
            </button>
          ))}
        </nav>

        <div className="grid gap-4 xl:grid-cols-[1.5fr_1fr]">
          <div className="space-y-4">
            <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
              <div className="mb-3 flex items-center justify-between gap-3">
                <h3 className="text-sm font-semibold">{t("settings.skillMarkdownPreview")}</h3>
                <Button
                  variant="secondary"
                  className="h-8 gap-1 border-[#e5e7eb] bg-white px-2.5 text-xs text-[#666]"
                  onClick={() => onOpenDir(skill.base_dir)}
                >
                  {t("settings.skillOpenInEditor")} <ExternalLink className="h-3 w-3" />
                </Button>
              </div>
              <div className="prose prose-sm max-w-none text-[13px] leading-7 text-[#374151] dark:prose-invert">
                <MarkdownRenderer content={skill.content} />
              </div>
            </section>

            <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
              <div className="mb-3 flex items-center justify-between gap-3">
                <h3 className="text-sm font-semibold">{t("settings.skillDiffTab")}</h3>
                <span className="text-xs text-[#888]">{t("settings.skillDiffNotLoaded")}</span>
              </div>
              <div className="rounded-md border border-[#f0f0f0] bg-[#fafafa] p-4 font-mono text-xs text-[#888]">
                {t("settings.skillDiffUnavailable")}
              </div>
            </section>
          </div>

          <aside className="space-y-4">
            <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
              <h3 className="mb-3 text-sm font-semibold">{t("settings.skillValidationTab")}</h3>
              <div className="space-y-2.5">
                {validationRows.map((row) => (
                  <div
                    key={row.label}
                    className="flex items-center justify-between gap-3 text-[13px]"
                  >
                    <span className="inline-flex items-center gap-2 text-[#374151]">
                      <File className="h-3.5 w-3.5 text-[#9ca3af]" />
                      {row.label}
                    </span>
                    <span
                      className={
                        row.warning
                          ? "text-xs font-medium text-[#f59e0b]"
                          : row.ok
                            ? "text-xs font-medium text-[#22c55e]"
                            : "text-xs font-medium text-[#ef4444]"
                      }
                    >
                      {row.warning
                        ? t("settings.skillValidationWarning", { detail: row.detail })
                        : row.ok
                          ? t("settings.skillValidationPassed")
                          : row.detail}
                    </span>
                  </div>
                ))}
              </div>
              <button
                type="button"
                className="mt-3 text-xs font-medium text-[#2563eb]"
                onClick={() => setActiveTab("validation")}
              >
                {t("settings.skillViewDetails")}
              </button>
              {requiresEnv.length > 0 && (
                <div className="mt-4 border-t border-[#f0f0f0] pt-3">
                  <h4 className="mb-2 text-xs font-semibold text-[#666]">
                    {t("settings.skillEnvVars")}
                  </h4>
                  <div className="space-y-2">
                    {requiresEnv.map((envKey) => renderEnvRow(envKey))}
                  </div>
                </div>
              )}
            </section>

            <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
              <h3 className="mb-3 text-sm font-semibold">{t("settings.skillInstallTab")}</h3>
              <div>
                {installStates.map((state) => (
                  <InstallLocationRow key={state.app} state={state} skillName={skill.name} />
                ))}
              </div>
            </section>

            <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
              <h3 className="mb-3 text-sm font-semibold">{t("settings.skillMetadata")}</h3>
              <dl className="space-y-2">
                <MetaRow
                  label={t("settings.skillsManager.detailVersion")}
                  value={skill.display?.version ?? "—"}
                />
                <MetaRow
                  label={t("settings.skillsManager.detailAuthor")}
                  value={skill.display?.author ?? skill.authored_by ?? "—"}
                />
                <MetaRow
                  label={t("settings.skillsManager.sortSource")}
                  value={skillSourceLabel(t, skill.source)}
                />
                <MetaRow
                  label={t("settings.skillsManager.sourceDir")}
                  value={compactPath(skill.base_dir)}
                />
                <MetaRow
                  label={t("settings.skillsManager.detailFiles")}
                  value={String(skill.files.length)}
                />
              </dl>
            </section>

            <div className="grid grid-cols-2 gap-3">
              <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
                <h3 className="mb-2 text-[13px] font-semibold">{t("settings.skillCompatibleApps")}</h3>
                <div className="flex gap-1.5">
                  {DETAIL_APPS.map((app) => (
                    <AppBadge key={app} app={app} />
                  ))}
                </div>
                <p className="mt-2 text-[11px] text-[#888]">
                  {t("settings.skillCompatibleCount", { count: DETAIL_APPS.length })}
                </p>
              </section>
              <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
                <h3 className="mb-1 text-[13px] font-semibold">{t("settings.skillRecentUsage")}</h3>
                <div>
                  <span className="text-2xl font-bold text-[#2563eb]">
                    {usage?.usageCount ?? 0}
                  </span>
                  <span className="ml-1 text-[13px] text-[#666]">{t("settings.skillTimes")}</span>
                </div>
                <p className="mt-1 text-[11px] text-[#888]">
                  {usage?.lastUsedAt ?? t("settings.skillsManager.neverUsed")}
                </p>
              </section>
            </div>

            {skill.install && skill.install.length > 0 && (
              <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
                <h3 className="mb-3 text-sm font-semibold">{t("settings.skillInstall")}</h3>
                <div className="space-y-2">
                  {skill.install.map((spec, idx) => (
                    <InstallSpecRow key={idx} spec={spec} skillName={skill.name} specIndex={idx} />
                  ))}
                </div>
              </section>
            )}

            {skill.files.length > 0 && (
              <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
                <h3 className="mb-3 text-sm font-semibold">{t("settings.skillFiles")}</h3>
                <div className="max-h-56 overflow-auto rounded-md border border-[#f0f0f0]">
                  {skill.files.map((file) => (
                    <div
                      key={file.name}
                      className="flex items-center gap-2 border-b border-[#f0f0f0] bg-[#fafafa] px-3 py-2 text-xs last:border-b-0"
                    >
                      {file.is_dir ? (
                        <Folder className="h-3.5 w-3.5 text-[#2563eb]" />
                      ) : (
                        <File className="h-3.5 w-3.5 text-[#9ca3af]" />
                      )}
                      <span className="min-w-0 flex-1 truncate text-[#374151]">
                        {file.name}
                        {file.is_dir ? "/" : ""}
                      </span>
                      {!file.is_dir && (
                        <span className="shrink-0 text-[#888]">{formatBytes(file.size)}</span>
                      )}
                    </div>
                  ))}
                </div>
              </section>
            )}
          </aside>
        </div>
      </div>
    </div>
  )

  function renderEnvRow(envKey: string) {
    const currentValue = envValues[envKey] ?? ""
    const isDirty = envDirty[envKey] ?? false
    const isSaving = envSaving[envKey] ?? false
    const isConfigured = envStatus[skill.name]?.[envKey] ?? false
    return (
      <div key={envKey} className="flex items-center gap-2">
        <span
          className={cn(
            "h-2 w-2 shrink-0 rounded-full",
            isConfigured ? "bg-[#22c55e]" : "bg-[#f59e0b]",
          )}
        />
        <IconTip label={envKey}>
          <code className="w-28 shrink-0 truncate text-xs text-[#374151]">{envKey}</code>
        </IconTip>
        <Input
          type="password"
          className="h-7 min-w-0 flex-1 border-[#e5e7eb] bg-white text-xs"
          placeholder={t("settings.skillEnvPlaceholder", { key: envKey })}
          value={currentValue}
          onChange={(e) => onEnvValueChange(envKey, e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && isDirty) onSaveEnvVar(envKey)
          }}
        />
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={() => isDirty && onSaveEnvVar(envKey)}
          disabled={!isDirty || isSaving}
        >
          <Check className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={() => currentValue && onRemoveEnvVar(envKey)}
          disabled={!currentValue}
        >
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
      </div>
    )
  }
}

function InstallLocationRow({
  state,
  skillName,
}: {
  state: SkillAppInstallState
  skillName: string
}) {
  const app = state.app as DetailAppKind
  return (
    <div className="flex items-center gap-3 border-b border-[#f0f0f0] py-2.5 last:border-0">
      <AppBadge app={app} />
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium text-[#374151]">{appName(app)}</div>
        <div className="mt-0.5 truncate text-[11px] text-[#888]">
          {state.targetPath || `${defaultSkillPath(app)}/${skillName}`}
        </div>
      </div>
      <span
        className={cn(
          "inline-flex items-center gap-1 text-xs",
          state.installed ? "text-[#22c55e]" : "text-[#9ca3af]",
        )}
      >
        <span
          className={cn("h-2 w-2 rounded-full", state.installed ? "bg-[#22c55e]" : "bg-[#d1d5db]")}
        />
        {state.installed ? t("settings.skillInstalled") : t("settings.skillNotInstalled")}
      </span>
      <span className="text-[#9ca3af]">⋯</span>
    </div>
  )
}

function MetaRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3 text-xs">
      <dt className="text-[#888]">{label}</dt>
      <dd className="min-w-0 truncate font-medium text-[#1d1d1f]">{value}</dd>
    </div>
  )
}

function AppBadge({ app }: { app: DetailAppKind }) {
  const palette: Record<DetailAppKind, string> = {
    claude: "bg-[#ffedd5] text-[#f97316]",
    codex: "bg-[#f3f4f6] text-[#6b7280]",
    gemini: "bg-[#dcfce7] text-[#22c55e]",
    opencode: "bg-[#dbeafe] text-[#2563eb]",
  }
  return (
    <span
      className={cn(
        "flex h-6 w-6 items-center justify-center rounded-md text-xs font-semibold",
        palette[app],
      )}
    >
      {appIcon(app)}
    </span>
  )
}

function defaultInstallState(app: DetailAppKind, skillName: string): SkillAppInstallState {
  return {
    app,
    installed: false,
    state: "external",
    targetPath: `${defaultSkillPath(app)}/${skillName}`,
  }
}

function defaultSkillPath(app: DetailAppKind): string {
  if (app === "claude") return "~/Library/Application Support/Claude/skills"
  if (app === "codex") return "~/.codex/skills"
  if (app === "gemini") return "~/Library/Application Support/Google/Gemini/skills"
  return "~/.opencode/skills"
}

function appName(app: DetailAppKind): string {
  if (app === "opencode") return "OpenCode"
  return app[0].toUpperCase() + app.slice(1)
}

function appIcon(app: DetailAppKind): string {
  if (app === "claude") return "☀"
  if (app === "gemini") return "✦"
  return "⬡"
}

function compactPath(path: string): string {
  return path.replace(/^C:\\Users\\[^\\]+/i, "~").replace(/^\/Users\/[^/]+/i, "~")
}
