import { useMemo, useState } from "react"
import { useTranslation } from "react-i18next"
import { skillSourceLabel } from "./skillSourceLabel"
import { cn } from "@/lib/utils"
import { IconTip } from "@/components/ui/tooltip"
import { Button } from "@/components/ui/button"
import { SearchInput } from "@/components/ui/search-input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Archive, Download, FolderOpen, RefreshCw, SendToBack, Trash2 } from "lucide-react"
import { logger } from "@/lib/logger"
import type { SkillStatusEntry, SkillSummary } from "../types"
import type { AppKind, SkillAppInstallState, SkillDockSnapshot, SkillUsageSnapshot } from "./types"
import { exportSkillZip, installSkillToApp, scanSkillUsage, uninstallManagedSkill } from "./api"

type SkillSourceFilter = "all" | "bundled" | "user"
type SkillStateFilter = "all" | "enabled" | "disabled" | "attention"
type SkillSortKey = "name" | "source" | "status"
type LocalSkillAppKind = Extract<AppKind, "claude" | "codex" | "opencode" | "gemini">
const LOCAL_SKILL_APPS: LocalSkillAppKind[] = ["claude", "codex", "opencode", "gemini"]

interface SkillListViewProps {
  skills: SkillSummary[]
  loading: boolean
  envStatus: Record<string, Record<string, boolean>>
  skillStatusByName: Record<string, SkillStatusEntry | undefined>
  dockSnapshot: SkillDockSnapshot | null
  onToggleSkill: (name: string, enabled: boolean) => void | Promise<void>
  onSelectSkill: (name: string) => void
  onOpenDir: (path: string) => void
  onAddDir: () => void
  search: string
  sourceFilter: SkillSourceFilter
  stateFilter: SkillStateFilter
  sortKey: SkillSortKey
  onSearchChange: (value: string) => void
  onSourceFilterChange: (value: SkillSourceFilter) => void
  onStateFilterChange: (value: SkillStateFilter) => void
  onSortKeyChange: (value: SkillSortKey) => void
  onReload: () => void | Promise<void>
}

export default function SkillListView({
  skills,
  loading,
  envStatus,
  skillStatusByName,
  dockSnapshot,
  onToggleSkill,
  onSelectSkill,
  onOpenDir,
  onAddDir,
  search,
  sourceFilter,
  stateFilter,
  sortKey,
  onSearchChange,
  onSourceFilterChange,
  onStateFilterChange,
  onSortKeyChange,
  onReload,
}: SkillListViewProps) {
  const { t } = useTranslation()
  const [selectedNames, setSelectedNames] = useState<Set<string>>(() => new Set())
  const [selectedApp, setSelectedApp] = useState<LocalSkillAppKind>("claude")
  const [actionBusy, setActionBusy] = useState<string | null>(null)
  const [usageRows, setUsageRows] = useState<SkillUsageSnapshot[] | null>(null)
  const [page, setPage] = useState(1)
  const [pageSize, setPageSize] = useState(20)

  const usageByName = useMemo(() => {
    const rows = usageRows ?? dockSnapshot?.usage ?? []
    return new Map(rows.map((row) => [row.skillName, row]))
  }, [dockSnapshot?.usage, usageRows])

  const packageByName = useMemo(() => {
    const packages = dockSnapshot?.packages ?? []
    return new Map(packages.map((pkg) => [pkg.name, pkg]))
  }, [dockSnapshot?.packages])

  function hasEnvWarning(skillName: string): boolean {
    const status = envStatus[skillName]
    if (!status) return false
    return Object.values(status).some((v) => !v)
  }

  function statusLabel(status?: SkillStatusEntry): string {
    if (!status) return t("settings.skillStatusEligible")
    const lines: string[] = []
    if (status.hard_blocked) {
      lines.push(t("settings.skillHardBlocked"))
      if (status.current_os || status.supported_os?.length) {
        lines.push(
          `${t("settings.skillCurrentOs")}: ${status.current_os || "?"}; ${t("settings.skillSupportedOs")}: ${
            status.supported_os?.join(", ") || "?"
          }`,
        )
      }
    } else if (status.needs_setup) {
      lines.push(t("settings.skillNeedsSetup"))
    } else if (status.eligible) {
      lines.push(t("settings.skillStatusEligible"))
    }
    if (status.missing_bins?.length)
      lines.push(`${t("settings.skillMissingBins")}: ${status.missing_bins.join(", ")}`)
    if (status.missing_any_bins?.length)
      lines.push(`${t("settings.skillMissingAnyBins")}: ${status.missing_any_bins.join(" | ")}`)
    if (status.missing_env?.length)
      lines.push(`${t("settings.skillMissingEnv")}: ${status.missing_env.join(", ")}`)
    if (status.missing_config?.length)
      lines.push(`${t("settings.skillMissingConfig")}: ${status.missing_config.join(", ")}`)
    return lines.join("\n")
  }

  const normalizedSearch = search.trim().toLowerCase()
  const filteredSkills = skills
    .filter((skill) => {
      if (sourceFilter === "bundled" && skill.source !== "bundled") return false
      if (sourceFilter === "user" && skill.source === "bundled") return false
      const status = skillStatusByName[skill.name]
      const attention = hasEnvWarning(skill.name) || !!status?.hard_blocked || !!status?.needs_setup
      if (stateFilter === "enabled" && !skill.enabled) return false
      if (stateFilter === "disabled" && skill.enabled) return false
      if (stateFilter === "attention" && !attention) return false
      if (!normalizedSearch) return true
      return [
        skill.name,
        skill.description,
        skill.source,
        skill.base_dir,
        skill.display?.author,
        skill.display?.version,
        ...(skill.display?.tags ?? []),
      ]
        .filter(Boolean)
        .join(" ")
        .toLowerCase()
        .includes(normalizedSearch)
    })
    .sort((left, right) => {
      if (sortKey === "source") {
        const bySource = left.source.localeCompare(right.source)
        if (bySource !== 0) return bySource
      }
      if (sortKey === "status") {
        const leftScore = skillSortStatusScore(
          left,
          skillStatusByName[left.name],
          hasEnvWarning(left.name),
        )
        const rightScore = skillSortStatusScore(
          right,
          skillStatusByName[right.name],
          hasEnvWarning(right.name),
        )
        if (leftScore !== rightScore) return rightScore - leftScore
      }
      return left.name.localeCompare(right.name)
    })

  const pageCount = Math.max(1, Math.ceil(filteredSkills.length / pageSize))
  const currentPage = Math.min(page, pageCount)
  const pageStart = (currentPage - 1) * pageSize
  const pageSkills = filteredSkills.slice(pageStart, pageStart + pageSize)
  const visibleSelectedCount = pageSkills.filter((skill) => selectedNames.has(skill.name)).length
  const allVisibleSelected = pageSkills.length > 0 && visibleSelectedCount === pageSkills.length
  const selectedSkills = skills.filter((skill) => selectedNames.has(skill.name))

  function appStateFor(skillName: string, app: LocalSkillAppKind): SkillAppInstallState {
    return (
      usageByName.get(skillName)?.apps.find((state) => state.app === app) ?? {
        app,
        installed: false,
        state: "external",
      }
    )
  }

  function compactSkillPath(path: string): string {
    const normalized = path.replace(/\\/g, "/")
    const homeSkillMatch = normalized.match(/(?:^|\/)\.hope-agent\/skills\/(.+)$/)
    if (homeSkillMatch) return `~/skills/${homeSkillMatch[1]}`
    const sharedSkillMatch = normalized.match(/(?:^|\/)\.agents\/skills\/(.+)$/)
    if (sharedSkillMatch) return `~/.agents/${sharedSkillMatch[1]}`
    const skillsMatch = normalized.match(/\/skills\/(.+)$/i)
    if (skillsMatch)
      return `~/skills/${skillsMatch[1].split("/").filter(Boolean).slice(-2).join("/")}`
    const parts = normalized.split("/").filter(Boolean)
    if (parts.length <= 2) return normalized
    return `…/${parts.slice(-2).join("/")}`
  }

  function skillIconFor(skill: SkillSummary): string {
    const explicitEmoji = skill.display?.emoji?.trim()
    if (explicitEmoji) return explicitEmoji

    const haystack = `${skill.name} ${skill.description} ${skill.base_dir}`.toLowerCase()
    const keywordIcons: Array<[RegExp, string]> = [
      [/\b(code|review|lint|refactor|debug|dev|api|sdk|typescript|rust)\b|代码|开发|审查/, "💻"],
      [/\b(prompt|lab|eval|experiment|model|llm)\b|提示词|模型|实验|评测/, "🧪"],
      [/\b(deploy|release|ship|ci|docker|kubernetes|ops)\b|部署|发布|运维/, "🚀"],
      [/\b(ui|ux|inspect|a11y|accessibility|design|visual)\b|界面|设计|可访问性|组件/, "👁️"],
      [/\b(doc|docs|writer|readme|markdown|spec)\b|文档|写作|说明/, "📝"],
      [/\b(test|vitest|jest|playwright|qa|suite)\b|测试|用例|验证/, "🧪"],
      [/\b(data|db|sql|analytics|report|metric)\b|数据|报表|指标/, "🗄️"],
      [/\b(team|meeting|collab|status|project)\b|团队|协作|会议|项目/, "👥"],
      [/\b(workflow|automation|agent|cron|task)\b|自动化|流程|任务/, "🤖"],
    ]
    for (const [pattern, icon] of keywordIcons) {
      if (pattern.test(haystack)) return icon
    }

    const fallbackIcons = ["💻", "🧪", "🚀", "👁️", "📝", "🎨", "🤖", "🗄️", "👥"]
    const hash = Array.from(skill.name).reduce(
      (total, char) => total + (char.codePointAt(0) ?? 0),
      0,
    )
    return fallbackIcons[hash % fallbackIcons.length]
  }

  function isManagedSkill(skill: SkillSummary): boolean {
    const pkg = packageByName.get(skill.name)
    if (pkg?.readOnly) return false
    return skill.source !== "bundled"
  }

  function toggleSelected(name: string, checked: boolean) {
    setSelectedNames((current) => {
      const next = new Set(current)
      if (checked) next.add(name)
      else next.delete(name)
      return next
    })
  }

  function toggleAllVisible(checked: boolean) {
    setSelectedNames((current) => {
      const next = new Set(current)
      for (const skill of pageSkills) {
        if (checked) next.add(skill.name)
        else next.delete(skill.name)
      }
      return next
    })
  }

  async function refreshAfterAction() {
    await onReload()
  }

  async function handleRefreshAll() {
    setActionBusy("refresh")
    try {
      await onReload()
      const report = await scanSkillUsage()
      setUsageRows(report.usage)
    } catch (error) {
      logger.error("settings", "SkillListView::refreshAll", "Failed to refresh skills", error)
    } finally {
      setActionBusy(null)
    }
  }

  async function handleExportSkills(names: string[]) {
    if (!names.length) return
    const outputPath = window.prompt(
      t("settings.skillsManager.exportPathPrompt", { count: names.length }),
      names.length === 1 ? `${names[0]}.zip` : "skills-export",
    )
    if (!outputPath?.trim()) return
    setActionBusy("export")
    try {
      for (const name of names) await exportSkillZip(name, outputPath.trim())
      await refreshAfterAction()
    } catch (error) {
      logger.error("settings", "SkillListView::export", "Failed to export skill", error)
      window.alert(error instanceof Error ? error.message : String(error))
    } finally {
      setActionBusy(null)
    }
  }

  async function handleInstallSkills(names: string[], app: LocalSkillAppKind) {
    if (!names.length) return
    const confirmed = window.confirm(
      t("settings.skillsManager.installToAppConfirm", {
        count: names.length,
        app: t(`settings.skillsDockExtensions.app.${app}`),
      }),
    )
    if (!confirmed) return
    setActionBusy("install")
    try {
      for (const name of names) await installSkillToApp(name, app)
      await refreshAfterAction()
    } catch (error) {
      logger.error("settings", "SkillListView::installToApp", "Failed to install skill", error)
      window.alert(error instanceof Error ? error.message : String(error))
    } finally {
      setActionBusy(null)
    }
  }

  async function handleDeleteSkills(items: SkillSummary[]) {
    const deletable = items.filter(isManagedSkill)
    if (!deletable.length) return
    const confirmed = window.confirm(
      t("settings.skillsManager.deleteConfirm", {
        count: deletable.length,
        names: deletable.map((skill) => skill.name).join(", "),
      }),
    )
    if (!confirmed) return
    setActionBusy("delete")
    try {
      for (const skill of deletable) await uninstallManagedSkill(skill.name)
      setSelectedNames((current) => {
        const next = new Set(current)
        for (const skill of deletable) next.delete(skill.name)
        return next
      })
      await refreshAfterAction()
    } catch (error) {
      logger.error("settings", "SkillListView::delete", "Failed to delete skill", error)
      window.alert(error instanceof Error ? error.message : String(error))
    } finally {
      setActionBusy(null)
    }
  }

  async function handleBulkToggle(enabled: boolean) {
    if (!selectedSkills.length) return
    setActionBusy(enabled ? "bulkEnable" : "bulkDisable")
    try {
      for (const skill of selectedSkills) await onToggleSkill(skill.name, enabled)
      await refreshAfterAction()
    } finally {
      setActionBusy(null)
    }
  }

  function renderAppStatus(skill: SkillSummary, app: LocalSkillAppKind) {
    const state = appStateFor(skill.name, app)
    const label = state.installed
      ? t("settings.skillsManager.installedShort")
      : t("settings.skillsManager.notInstalled")
    const detail = [
      t(`settings.skillsDockExtensions.app.${app}`),
      state.installed ? t(`settings.skillsManager.appInstallState.${state.state}`) : label,
      state.reason,
      state.targetPath,
    ]
      .filter(Boolean)
      .join(" · ")
    return (
      <td key={`${skill.name}:${app}`} className="px-2 py-3 text-center align-middle">
        <IconTip label={detail}>
          <span className="inline-flex cursor-help items-center justify-center">
            {state.installed ? (
              <span className="inline-flex h-[18px] w-[18px] items-center justify-center rounded-full bg-[#22c55e] text-[11px] font-semibold text-white">
                ✓
              </span>
            ) : (
              <span className="inline-flex h-[18px] w-[18px] items-center justify-center rounded-full bg-[#e5e7eb] text-[12px] font-semibold text-[#9ca3af]">
                −
              </span>
            )}
          </span>
        </IconTip>
      </td>
    )
  }

  return (
    <div className="flex-1 min-h-0 overflow-y-auto bg-[#f5f5f7] p-5 text-[#1d1d1f] dark:bg-background dark:text-foreground">
      <section className="min-w-0 space-y-4">
        <div className="flex flex-col gap-3 xl:flex-row xl:items-center xl:justify-between">
          <div className="flex min-w-0 flex-1 flex-col gap-2 lg:flex-row lg:items-center">
            <SearchInput
              value={search}
              onChange={(event) => onSearchChange(event.currentTarget.value)}
              placeholder={t("settings.skillsManager.searchPlaceholder")}
              className="h-9 max-w-[320px] border-[#e5e7eb] bg-white text-[13px] shadow-none"
            />
            <div className="grid gap-2 sm:grid-cols-3">
              <Select
                value={stateFilter}
                onValueChange={(value) => onStateFilterChange(value as SkillStateFilter)}
              >
                <SelectTrigger className="h-9 min-w-[8rem] border-[#e5e7eb] bg-white text-[13px] text-[#374151] shadow-none">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">{t("settings.skillsManager.stateAll")}</SelectItem>
                  <SelectItem value="enabled">
                    {t("settings.skillsManager.stateEnabled")}
                  </SelectItem>
                  <SelectItem value="disabled">
                    {t("settings.skillsManager.stateDisabled")}
                  </SelectItem>
                  <SelectItem value="attention">
                    {t("settings.skillsManager.stateAttention")}
                  </SelectItem>
                </SelectContent>
              </Select>
              <Select
                value={sourceFilter}
                onValueChange={(value) => onSourceFilterChange(value as SkillSourceFilter)}
              >
                <SelectTrigger className="h-9 min-w-[8rem] border-[#e5e7eb] bg-white text-[13px] text-[#374151] shadow-none">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">{t("settings.skillsManager.sourceAll")}</SelectItem>
                  <SelectItem value="bundled">
                    {t("settings.skillsManager.sourceBundled")}
                  </SelectItem>
                  <SelectItem value="user">{t("settings.skillsManager.sourceUser")}</SelectItem>
                </SelectContent>
              </Select>
              <Select
                value={sortKey}
                onValueChange={(value) => onSortKeyChange(value as SkillSortKey)}
              >
                <SelectTrigger className="h-9 min-w-[8rem] border-[#e5e7eb] bg-white text-[13px] text-[#374151] shadow-none">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="name">{t("settings.skillsManager.sortName")}</SelectItem>
                  <SelectItem value="source">{t("settings.skillsManager.sortSource")}</SelectItem>
                  <SelectItem value="status">{t("settings.skillsManager.sortStatus")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-2">
            <Select
              value={String(pageSize)}
              onValueChange={(value) => {
                setPageSize(Number(value))
                setPage(1)
              }}
            >
              <SelectTrigger className="h-9 w-[7.5rem] border-[#e5e7eb] bg-white text-[13px] shadow-none">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="10">10 / page</SelectItem>
                <SelectItem value="20">20 / page</SelectItem>
                <SelectItem value="50">50 / page</SelectItem>
              </SelectContent>
            </Select>
            <Button
              variant="secondary"
              className="h-9 gap-1.5 border-[#e5e7eb] bg-white px-3.5 text-[13px] text-[#374151] shadow-none"
              disabled={actionBusy === "refresh"}
              onClick={() => void handleRefreshAll()}
            >
              <RefreshCw className="h-3.5 w-3.5" />
              {t("settings.skillsManager.reload")}
            </Button>
            <Button
              className="h-9 gap-1.5 bg-[#2563eb] px-4 text-[13px] text-white shadow-none hover:bg-[#1d4ed8]"
              onClick={onAddDir}
            >
              <FolderOpen className="h-3.5 w-3.5" />
              {t("settings.skillsDirAdd")}
            </Button>
          </div>
        </div>

        <div className="flex flex-col gap-3 rounded-[10px] bg-white px-4 py-3 text-[13px] shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card lg:flex-row lg:items-center lg:justify-between">
          <label className="flex items-center gap-3 text-[#1d1d1f]">
            <input
              type="checkbox"
              className="h-4 w-4 rounded-[3px] border-[#d1d5db] accent-[#2563eb]"
              checked={allVisibleSelected}
              onChange={(event) => toggleAllVisible(event.currentTarget.checked)}
              aria-label={t("settings.skillsManager.selectAll")}
            />
            <span>
              已选择 {selectedNames.size} 项（共 {filteredSkills.length} 项）
            </span>
          </label>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              size="sm"
              variant="secondary"
              className="h-8 gap-1.5 border-[#e5e7eb] bg-white px-3 text-xs text-[#374151] shadow-none"
              disabled={!selectedNames.size || actionBusy === "export"}
              onClick={() => void handleExportSkills(selectedSkills.map((skill) => skill.name))}
            >
              <Download className="h-3.5 w-3.5" />
              {t("settings.skillsManager.export")}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              className="h-8 border-[#e5e7eb] bg-white px-3 text-xs text-[#374151] shadow-none"
              disabled={!selectedNames.size || actionBusy === "bulkEnable"}
              onClick={() => void handleBulkToggle(true)}
            >
              {t("settings.skillsManager.stateEnabled")}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              className="h-8 border-[#e5e7eb] bg-white px-3 text-xs text-[#374151] shadow-none"
              disabled={!selectedNames.size || actionBusy === "bulkDisable"}
              onClick={() => void handleBulkToggle(false)}
            >
              {t("settings.skillsManager.stateDisabled")}
            </Button>
            <Select
              value={selectedApp}
              onValueChange={(value) => setSelectedApp(value as LocalSkillAppKind)}
            >
              <SelectTrigger className="h-8 w-[9.5rem] border-[#e5e7eb] bg-white text-xs text-[#374151] shadow-none">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {LOCAL_SKILL_APPS.map((app) => (
                  <SelectItem key={app} value={app}>
                    {t(`settings.skillsDockExtensions.app.${app}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button
              size="sm"
              variant="secondary"
              className="h-8 gap-1.5 border-[#e5e7eb] bg-white px-3 text-xs text-[#374151] shadow-none"
              disabled={!selectedNames.size || actionBusy === "install"}
              onClick={() =>
                void handleInstallSkills(
                  selectedSkills.map((skill) => skill.name),
                  selectedApp,
                )
              }
            >
              <SendToBack className="h-3.5 w-3.5" />
              {t("settings.skillsManager.installSelectedToApp")}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              className="h-8 gap-1.5 border-[#e5e7eb] bg-white px-3 text-xs text-[#374151] shadow-none"
              disabled={!selectedNames.size || actionBusy === "delete"}
              onClick={() => void handleDeleteSkills(selectedSkills)}
            >
              <Trash2 className="h-3.5 w-3.5" />
              {t("settings.skillsManager.delete")}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              className="h-8 border-[#e5e7eb] bg-white px-3 text-xs text-[#374151] shadow-none"
              disabled={!selectedNames.size}
              onClick={() => setSelectedNames(new Set())}
            >
              ⋯
            </Button>
          </div>
        </div>

        {loading ? (
          <div className="flex items-center justify-center rounded-[10px] bg-white py-12 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
            <div className="h-5 w-5 animate-spin rounded-full border-2 border-[#1d1d1f] border-t-transparent" />
          </div>
        ) : filteredSkills.length === 0 ? (
          <div className="rounded-[10px] bg-white py-12 text-center shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
            <Archive className="mx-auto mb-3 h-10 w-10 text-[#d1d5db]" />
            <p className="text-sm text-[#666]">
              {skills.length === 0 ? t("settings.noSkills") : t("settings.skillsManager.noMatches")}
            </p>
            <p className="mt-1 text-xs text-[#888]">
              {skills.length === 0
                ? t("settings.noSkillsHint")
                : t("settings.skillsManager.noMatchesHint")}
            </p>
          </div>
        ) : (
          <div className="overflow-hidden rounded-[10px] bg-white shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
            <div className="overflow-x-auto">
              <table className="w-full min-w-[58rem] border-collapse text-left text-[13px]">
                <thead className="whitespace-nowrap text-xs font-medium text-[#888]">
                  <tr className="border-b border-[#f0f0f0]">
                    <th rowSpan={2} className="w-10 px-3 py-2 align-middle">
                      <input
                        type="checkbox"
                        className="h-4 w-4 rounded-[3px] border-[#d1d5db] accent-[#2563eb]"
                        checked={allVisibleSelected}
                        onChange={(event) => toggleAllVisible(event.currentTarget.checked)}
                        aria-label={t("settings.skillsManager.selectAll")}
                      />
                    </th>
                    <th rowSpan={2} className="w-[13rem] whitespace-nowrap px-3 py-2 align-middle">
                      {t("settings.skillsDockExtensions.skillColumn")}
                    </th>
                    <th rowSpan={2} className="w-[15rem] whitespace-nowrap px-3 py-2 align-middle">
                      {t("settings.skillsDockExtensions.descriptionColumn")}
                    </th>
                    <th rowSpan={2} className="w-24 whitespace-nowrap px-3 py-2 align-middle">
                      {t("settings.skillsManager.sortSource")}
                    </th>
                    <th rowSpan={2} className="w-[11rem] whitespace-nowrap px-3 py-2 align-middle">
                      {t("settings.skillsManager.sourceDir")}
                    </th>
                    <th rowSpan={2} className="w-24 whitespace-nowrap px-3 py-2 align-middle">
                      {t("settings.skillsManager.sortStatus")}
                    </th>
                    <th
                      colSpan={LOCAL_SKILL_APPS.length}
                      className="whitespace-nowrap px-3 py-2 text-center"
                    >
                      {t("settings.skillsManager.installStatus")}
                    </th>
                    <th
                      rowSpan={2}
                      className="w-20 whitespace-nowrap px-3 py-2 text-right align-middle"
                    >
                      {t("settings.skillsManager.usageCount")}
                    </th>
                  </tr>
                  <tr className="border-b border-[#f0f0f0] text-[11px]">
                    {LOCAL_SKILL_APPS.map((app) => (
                      <th key={app} className="w-16 whitespace-nowrap px-2 py-2 text-center">
                        {t(`settings.skillsDockExtensions.app.${app}`)}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {pageSkills.map((skill) => {
                    const showWarning = hasEnvWarning(skill.name)
                    const status = skillStatusByName[skill.name]
                    const hardBlocked = !!status?.hard_blocked
                    const needsSetup = !!status?.needs_setup && !hardBlocked
                    const attention = hardBlocked || needsSetup || showWarning
                    const display = skill.display
                    const usage = usageByName.get(skill.name)
                    const packageSummary = packageByName.get(skill.name)
                    return (
                      <tr
                        key={skill.name}
                        className="border-b border-[#f0f0f0] transition-colors last:border-0 hover:bg-[#fafafa]"
                      >
                        <td className="px-3 py-3 align-middle">
                          <input
                            type="checkbox"
                            className="h-4 w-4 rounded-[3px] border-[#d1d5db] accent-[#2563eb]"
                            checked={selectedNames.has(skill.name)}
                            onChange={(event) =>
                              toggleSelected(skill.name, event.currentTarget.checked)
                            }
                            aria-label={t("settings.skillsManager.selectSkill", {
                              name: skill.name,
                            })}
                          />
                        </td>
                        <td className="px-3 py-3 align-middle">
                          <Button
                            variant="ghost"
                            className="h-auto max-w-full gap-2 px-0 py-0 text-left text-[13px] font-medium text-[#1d1d1f] hover:bg-transparent"
                            onClick={() => onSelectSkill(skill.name)}
                          >
                            <span className="text-base">{skillIconFor(skill)}</span>
                            <span className={cn("truncate", !skill.enabled && "line-through")}>
                              {skill.name}
                            </span>
                          </Button>
                        </td>
                        <td className="px-3 py-3 align-middle">
                          <div className="line-clamp-2 text-xs leading-4 text-[#888]">
                            {skill.description || compactSkillPath(skill.base_dir)}
                          </div>
                          <div className="mt-1 flex flex-wrap items-center gap-1">
                            {display?.version && (
                              <span className="rounded bg-[#f5f5f7] px-1.5 py-0.5 text-[10px] text-[#888]">
                                v{display.version}
                              </span>
                            )}
                            {packageSummary?.version &&
                              packageSummary.version !== display?.version && (
                                <span className="rounded bg-[#f5f5f7] px-1.5 py-0.5 text-[10px] text-[#888]">
                                  {packageSummary.version}
                                </span>
                              )}
                            {skill.always && (
                              <span className="rounded bg-[#dcfce7] px-1.5 py-0.5 text-[10px] text-[#166534]">
                                {t("settings.skillSkipsRequirements")}
                              </span>
                            )}
                          </div>
                        </td>
                        <td className="px-3 py-3 align-middle">
                          <span className="rounded-full bg-[#f5f5f7] px-2 py-0.5 text-[11px] font-medium text-[#666]">
                            {skillSourceLabel(t, skill.source)}
                          </span>
                        </td>
                        <td className="px-3 py-3 align-middle">
                          <IconTip label={skill.base_dir}>
                            <button
                              type="button"
                              className="max-w-[10rem] truncate text-left font-mono text-xs text-[#888] hover:text-[#1d1d1f]"
                              onClick={() => onOpenDir(skill.base_dir)}
                            >
                              {compactSkillPath(skill.base_dir)}
                            </button>
                          </IconTip>
                        </td>
                        <td className="px-3 py-3 align-middle">
                          <IconTip label={statusLabel(status)}>
                            <button
                              type="button"
                              className={cn(
                                "inline-flex cursor-help items-center gap-1.5 text-xs font-medium",
                                attention
                                  ? "text-[#d97706]"
                                  : skill.enabled
                                    ? "text-[#16a34a]"
                                    : "text-[#888]",
                              )}
                              onClick={() => void onToggleSkill(skill.name, !skill.enabled)}
                            >
                              <span
                                className={cn(
                                  "inline-flex h-[18px] w-[18px] items-center justify-center rounded-full text-[11px] font-semibold text-white",
                                  attention
                                    ? "bg-[#f59e0b]"
                                    : skill.enabled
                                      ? "bg-[#22c55e]"
                                      : "bg-[#d1d5db]",
                                )}
                              >
                                {attention ? "!" : skill.enabled ? "✓" : "−"}
                              </span>
                              {attention
                                ? t("settings.skillsManager.stateAttention")
                                : skill.enabled
                                  ? t("settings.skillStatusEligible")
                                  : t("settings.skillsManager.stateDisabled")}
                            </button>
                          </IconTip>
                        </td>
                        {LOCAL_SKILL_APPS.map((app) => renderAppStatus(skill, app))}
                        <td className="px-3 py-3 text-right align-middle text-[13px] font-medium text-[#1d1d1f]">
                          <IconTip
                            label={usage?.lastUsedAt ?? t("settings.skillsManager.neverUsed")}
                          >
                            <span className="cursor-help">{usage?.usageCount ?? 0}</span>
                          </IconTip>
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
          </div>
        )}

        {filteredSkills.length > pageSize && (
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-[10px] bg-white px-4 py-3 text-xs text-[#888] shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
            <span>
              {t("settings.skillsManager.pageSummary", {
                start: filteredSkills.length ? pageStart + 1 : 0,
                end: Math.min(pageStart + pageSize, filteredSkills.length),
                total: filteredSkills.length,
              })}
            </span>
            <div className="flex items-center gap-2">
              <Button
                variant="secondary"
                size="sm"
                className="h-7 border-[#e5e7eb] bg-white text-xs text-[#374151] shadow-none"
                disabled={currentPage <= 1}
                onClick={() => setPage((value) => Math.max(1, value - 1))}
              >
                {t("settings.skillsManager.prevPage")}
              </Button>
              <span className="min-w-16 text-center">
                {currentPage} / {pageCount}
              </span>
              <Button
                variant="secondary"
                size="sm"
                className="h-7 border-[#e5e7eb] bg-white text-xs text-[#374151] shadow-none"
                disabled={currentPage >= pageCount}
                onClick={() => setPage((value) => Math.min(pageCount, value + 1))}
              >
                {t("settings.skillsManager.nextPage")}
              </Button>
            </div>
          </div>
        )}
      </section>
    </div>
  )
}

function skillSortStatusScore(
  skill: SkillSummary,
  status: SkillStatusEntry | undefined,
  envWarning: boolean,
): number {
  if (status?.hard_blocked) return 4
  if (status?.needs_setup || envWarning) return 3
  if (!skill.enabled) return 2
  return 1
}
