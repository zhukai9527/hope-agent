import { useEffect, useMemo, useState, type ReactNode } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { IconTip } from "@/components/ui/tooltip"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  CheckCircle2,
  Clock3,
  Download,
  FolderOpen,
  Pencil,
  RotateCw,
  Trash2,
  Upload,
} from "lucide-react"
import type { SkillSummary } from "../types"
import type {
  AppKind,
  SkillDockSnapshot,
  SkillMarketHubConfigFile,
  SkillPublishDraft,
  SkillRegistryEntry,
  SkillRegistrySnapshot,
  SkillRemoteMarketEntry,
  SkillRemoteMarketSnapshot,
  SkillUsageAppBreakdown,
  SkillUsageRecentRecord,
  SkillUsageSnapshot,
  SkillUsageTrendPoint,
  SkillZipDryRunReport,
} from "./types"
import {
  clearSkillMarketHubToken,
  createSkillPublishDraft,
  dryRunImportSkillZip,
  exportSkillZip,
  importSkillZip,
  importSkillZipRenamed,
  installRegistrySkill,
  installRemoteMarketSkill,
  loadSkillMarketHubConfig,
  loadSkillMarketHubTokenStatus,
  pushSkillToMarketHub,
  scanSkillUsage,
  setSkillMarketHubEnabled,
  setSkillMarketHubToken,
  updateRegistrySkill,
  updateRemoteMarketSkill,
  upsertSkillMarketHub,
} from "./api"

type DockSection = "overview" | "local" | "market" | "io" | "usage" | "settings"

interface SkillDockExtensionsViewProps {
  skills: SkillSummary[]
  extraDirs: string[]
  localSkillsContent?: ReactNode
  initialSection?: DockSection
  lockedSection?: DockSection
  snapshot: SkillDockSnapshot | null
  registry: SkillRegistrySnapshot | null
  market: SkillRemoteMarketSnapshot | null
  marketLoading: boolean
  marketError: string | null
  marketSourceUrls: string[]
  onMarketSourceUrlsChange: (urls: string[]) => void
  onRefreshMarket: () => void
  onQuickImport: () => void
  onAddDir: () => void
  onImported: () => void
  onRefresh: () => void
}

const APP_KINDS: AppKind[] = ["hope", "claude", "codex", "gemini", "opencode"]
const EXTERNAL_APPS = ["claude", "codex", "gemini", "opencode"] as const
const APP_BAR_COLORS: Partial<Record<AppKind, string>> = {
  hope: "#64748b",
  claude: "#f97316",
  codex: "#9ca3af",
  gemini: "#22c55e",
  opencode: "#3b82f6",
}
const APP_BAR_CLASS_NAMES: Partial<Record<AppKind, string>> = {
  hope: "bg-slate-500",
  claude: "bg-orange-500",
  codex: "bg-zinc-400",
  gemini: "bg-green-500",
  opencode: "bg-blue-500",
}
const APP_TEXT_CLASS_NAMES: Partial<Record<AppKind, string>> = {
  hope: "text-slate-500",
  claude: "text-orange-500",
  codex: "text-zinc-400",
  gemini: "text-green-500",
  opencode: "text-blue-500",
}

const DOCK_SECTIONS: Array<{ id: DockSection; labelKey: string; descKey: string }> = [
  {
    id: "overview",
    labelKey: "settings.skillsDockExtensions.nav.overview",
    descKey: "settings.skillsDockExtensions.navDesc.overview",
  },
  {
    id: "local",
    labelKey: "settings.skillsDockExtensions.nav.localSkills",
    descKey: "settings.skillsDockExtensions.navDesc.localSkills",
  },
  {
    id: "io",
    labelKey: "settings.skillsDockExtensions.nav.importExport",
    descKey: "settings.skillsDockExtensions.navDesc.importExport",
  },
  {
    id: "usage",
    labelKey: "settings.skillsDockExtensions.nav.usage",
    descKey: "settings.skillsDockExtensions.navDesc.usage",
  },
]

export default function SkillDockExtensionsView({
  skills,
  extraDirs,
  localSkillsContent,
  initialSection = "overview",
  lockedSection,
  snapshot,
  registry,
  market,
  marketLoading,
  marketError,
  marketSourceUrls,
  onMarketSourceUrlsChange,
  onRefreshMarket,
  onQuickImport,
  onAddDir,
  onImported,
  onRefresh,
}: SkillDockExtensionsViewProps) {
  const { t } = useTranslation()
  const [activeSection, setActiveSection] = useState<DockSection>(initialSection)
  const visibleSection = lockedSection ?? activeSection
  const [usageRows, setUsageRows] = useState<SkillUsageSnapshot[] | null>(null)
  const [usageTrendRows, setUsageTrendRows] = useState<SkillUsageTrendPoint[] | null>(null)
  const [usageRecentRows, setUsageRecentRows] = useState<SkillUsageRecentRecord[] | null>(null)
  const [usageAppRows, setUsageAppRows] = useState<SkillUsageAppBreakdown[] | null>(null)
  const [usageAppFilter, setUsageAppFilter] = useState("all")
  const [usageSkillFilter, setUsageSkillFilter] = useState("all")
  const [usageGranularity, setUsageGranularity] = useState<"day" | "week">("day")
  const [actionBusy, setActionBusy] = useState<string | null>(null)
  const [hubConfig, setHubConfig] = useState<SkillMarketHubConfigFile | null>(null)
  const [hubLoading, setHubLoading] = useState(false)
  const [hubError, setHubError] = useState<string | null>(null)
  const [tokenStatuses, setTokenStatuses] = useState<Record<string, string>>({})
  const [tokenDrafts, setTokenDrafts] = useState<Record<string, string>>({})
  const [hubForm, setHubForm] = useState({
    id: "",
    name: "",
    baseUrl: "",
    kind: "skillhub" as const,
    sourceType: "registry" as const,
  })
  const [marketSourceDraft, setMarketSourceDraft] = useState("")
  const [zipPath, setZipPath] = useState("")
  const [zipRename, setZipRename] = useState("")
  const [zipReport, setZipReport] = useState<SkillZipDryRunReport | null>(null)
  const [zipError, setZipError] = useState<string | null>(null)
  const [exportSkillName, setExportSkillName] = useState("")
  const [exportPath, setExportPath] = useState("")
  const [publishSkillName, setPublishSkillName] = useState("")
  const [publishHubId, setPublishHubId] = useState("")
  const [publishDraft, setPublishDraft] = useState<SkillPublishDraft | null>(null)
  const [marketSearch, setMarketSearch] = useState("")
  const [marketSourceFilter, setMarketSourceFilter] = useState("all")
  const [marketSort, setMarketSort] = useState("recommended")

  const snapshotPackages = snapshot?.packages ?? []
  const snapshotSources = snapshot?.sources ?? []
  const snapshotApps = snapshot?.apps ?? []
  const snapshotUsage = useMemo(() => snapshot?.usage ?? [], [snapshot?.usage])
  const snapshotUsageTrend = useMemo(() => snapshot?.usageTrend ?? [], [snapshot?.usageTrend])
  const snapshotRecentUsage = useMemo(() => snapshot?.recentUsage ?? [], [snapshot?.recentUsage])
  const snapshotAppBreakdown = useMemo(
    () => snapshot?.usageAppBreakdown ?? [],
    [snapshot?.usageAppBreakdown],
  )
  const marketEntries = market?.entries ?? []
  const registryEntries = registry?.entries ?? []
  const hubs = hubConfig?.hubs ?? []
  const writableHubs = hubs.filter((hub) => !hub.readOnly)

  const matrixRows = useMemo<SkillUsageSnapshot[]>(() => {
    if (usageRows) return usageRows
    if (snapshotUsage.length) return snapshotUsage
    return skills.map((skill) => ({
      skillName: skill.name,
      usageCount: 0,
      lastUsedAt: null,
      apps: APP_KINDS.map((app) => ({
        app,
        installed: app === "hope" && skill.enabled,
        state: app === "hope" && skill.enabled ? "ready" : "external",
      })),
    }))
  }, [skills, snapshotUsage, usageRows])

  const usageDashboardRows = useMemo(
    () =>
      matrixRows.slice().sort((left, right) => {
        const byUsage = right.usageCount - left.usageCount
        return byUsage !== 0 ? byUsage : left.skillName.localeCompare(right.skillName)
      }),
    [matrixRows],
  )
  const rawUsageTrendRows = usageTrendRows ?? snapshotUsageTrend
  const rawUsageRecentRows = usageRecentRows ?? snapshotRecentUsage
  const rawUsageAppRows = usageAppRows ?? snapshotAppBreakdown
  const filteredUsageRows = useMemo(
    () =>
      usageDashboardRows.filter((row) =>
        usageSkillFilter === "all" ? true : row.skillName === usageSkillFilter,
      ),
    [usageDashboardRows, usageSkillFilter],
  )
  const filteredRecentUsageRows = useMemo(
    () =>
      rawUsageRecentRows.filter((row) => {
        if (usageAppFilter !== "all" && row.app !== usageAppFilter) return false
        if (usageSkillFilter !== "all" && row.skillName !== usageSkillFilter) return false
        return true
      }),
    [rawUsageRecentRows, usageAppFilter, usageSkillFilter],
  )
  const filteredUsageTrendRows = useMemo(
    () =>
      rawUsageTrendRows.filter((row) => {
        if (usageAppFilter !== "all" && row.app !== usageAppFilter) return false
        if (usageSkillFilter === "all") return true
        return filteredRecentUsageRows.some(
          (recent) =>
            recent.skillName === usageSkillFilter
            && recent.app === row.app
            && recent.activatedAt.startsWith(row.date),
        )
      }),
    [filteredRecentUsageRows, rawUsageTrendRows, usageAppFilter, usageSkillFilter],
  )
  const filteredUsageAppRows = useMemo(() => {
    if (usageAppFilter !== "all") {
      return rawUsageAppRows.filter((row) => row.app === usageAppFilter)
    }
    if (usageSkillFilter === "all") return rawUsageAppRows
    const counts = new Map<AppKind, number>()
    for (const row of filteredRecentUsageRows) counts.set(row.app, (counts.get(row.app) ?? 0) + row.count)
    return Array.from(counts.entries()).map(([app, count]) => ({ app, count }))
  }, [filteredRecentUsageRows, rawUsageAppRows, usageAppFilter, usageSkillFilter])

  const stats = useMemo(() => {
    const enabled = skills.filter((skill) => skill.enabled).length
    const bundled = skills.filter((skill) => skill.source === "bundled").length
    return {
      total: snapshotPackages.length || skills.length,
      enabled,
      bundled,
      user: skills.length - bundled,
      dirs: snapshotSources.length || extraDirs.length + 2,
      usage: matrixRows.reduce((total, row) => total + row.usageCount, 0),
    }
  }, [extraDirs.length, matrixRows, skills, snapshotPackages.length, snapshotSources.length])

  const appInstallCounts = APP_KINDS.map((app) => ({
    app,
    installed: matrixRows.filter((row) =>
      row.apps.some((state) => state.app === app && state.installed),
    ).length,
  }))
  const externalAppInstallCounts = appInstallCounts.filter(({ app }) => app !== "hope")
  const installedAnywhereCount = matrixRows.filter((row) =>
    row.apps.some((state) => state.app !== "hope" && state.installed),
  ).length
  const notInstalledAnywhereCount = Math.max(0, matrixRows.length - installedAnywhereCount)
  const failedInstallCount = matrixRows.filter((row) =>
    row.apps.some((state) => state.state === "conflict" || state.state === "attention"),
  ).length
  const activeUsageRows = matrixRows.filter((row) => row.usageCount > 0)
  const generatedAtLabel = snapshot?.generatedAt
    ? new Date(snapshot.generatedAt).toLocaleString()
    : "-"
  const verificationLabel = failedInstallCount
    ? t("settings.skillsDockExtensions.usageScanWarning", { count: failedInstallCount })
    : t("settings.skillsDockExtensions.usageScanPassed")
  const usageTrendPoints = buildUsageTrendPoints(matrixRows)
  const maxTrendCount = Math.max(1, ...usageTrendPoints.map((point) => point.count))
  const topFiveUsageRows = usageDashboardRows.slice(0, 5)
  const recentTimelineRows = buildUsageTimelineRows(matrixRows, generatedAtLabel).slice(0, 3)
  const settingsAppRows = EXTERNAL_APPS.map((app) => {
    const probe = snapshotApps.find((item) => item.app === app)
    return {
      app,
      skillPath: probe?.rootPath ?? defaultSkillPath(app),
      logPath: defaultLogPath(app),
      scanned: probe?.installed ?? false,
    }
  })
  const customSkillDirs = extraDirs.length
    ? extraDirs
    : snapshotSources
        .filter((source) => source.sourceType !== "bundled")
        .map((source) => source.rootPath)

  useEffect(() => {
    if (!skills.length) return
    if (!exportSkillName || !skills.some((skill) => skill.name === exportSkillName))
      setExportSkillName(skills[0]?.name ?? "")
    if (!publishSkillName || !skills.some((skill) => skill.name === publishSkillName))
      setPublishSkillName(skills[0]?.name ?? "")
  }, [exportSkillName, publishSkillName, skills])

  useEffect(() => {
    void refreshHubConfig()
  }, [])

  useEffect(() => {
    if (!publishHubId && writableHubs[0]) setPublishHubId(writableHubs[0].id)
  }, [publishHubId, writableHubs])

  function handleAddMarketSource() {
    const value = marketSourceDraft.trim()
    if (!value || marketSourceUrls.includes(value) || marketSourceUrls.length >= 5) return
    onMarketSourceUrlsChange([...marketSourceUrls, value])
    setMarketSourceDraft("")
  }

  function handleRemoveMarketSource(url: string) {
    onMarketSourceUrlsChange(marketSourceUrls.filter((item) => item !== url))
  }

  async function refreshHubConfig() {
    setHubLoading(true)
    setHubError(null)
    try {
      const config = await loadSkillMarketHubConfig()
      setHubConfig(config)
      const statuses = await Promise.all(
        config.hubs.map(
          async (hub) => [hub.id, await loadSkillMarketHubTokenStatus(hub.id)] as const,
        ),
      )
      setTokenStatuses(
        Object.fromEntries(
          statuses.map(([id, status]) => [id, status.masked || (status.hasToken ? "••••" : "")]),
        ),
      )
    } catch (error) {
      setHubError(error instanceof Error ? error.message : String(error))
    } finally {
      setHubLoading(false)
    }
  }

  async function runAction(label: string, fn: () => Promise<string | void>, alertResult = true) {
    setActionBusy(label)
    try {
      const message = await fn()
      if (alertResult && message) window.alert(message)
      return message
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      window.alert(message)
      throw error
    } finally {
      setActionBusy(null)
    }
  }

  async function handleScanUsage() {
    await runAction(
      "scanUsage",
      async () => {
        const report = await scanSkillUsage()
        setUsageRows(report.usage)
        setUsageTrendRows(report.usageTrend)
        setUsageRecentRows(report.recentUsage)
        setUsageAppRows(report.usageAppBreakdown)
      },
      false,
    )
  }

  async function handleMarketAction(entry: SkillRemoteMarketEntry) {
    const isUpdate = entry.updateAvailable
    const confirmed = window.confirm(
      isUpdate
        ? t("settings.skillsDockExtensions.remoteUpdateConfirm", { name: entry.name })
        : t("settings.skillsDockExtensions.remoteInstallConfirm", { name: entry.name }),
    )
    if (!confirmed) return
    await runAction(`market:${entry.id}`, async () => {
      const report = isUpdate
        ? await updateRemoteMarketSkill(entry)
        : await installRemoteMarketSkill(entry)
      onRefreshMarket()
      return isUpdate
        ? t("settings.skillsDockExtensions.remoteUpdateResult", { name: report.name })
        : t("settings.skillsDockExtensions.remoteInstallResult", { name: report.name })
    })
  }

  async function handleHubUpsert() {
    if (!hubForm.name.trim() || !hubForm.baseUrl.trim()) return
    await runAction(
      "hub:upsert",
      async () => {
        const config = await upsertSkillMarketHub({
          id: hubForm.id.trim() || undefined,
          name: hubForm.name.trim(),
          baseUrl: hubForm.baseUrl.trim(),
          kind: hubForm.kind,
          sourceType: hubForm.sourceType,
          readOnly: false,
          enabled: true,
        })
        setHubConfig(config)
        setHubForm({ id: "", name: "", baseUrl: "", kind: "skillhub", sourceType: "registry" })
        return t("settings.skillsDockExtensions.hubSaved")
      },
      false,
    )
  }

  async function handleTokenSave(hubId: string) {
    const token = tokenDrafts[hubId]?.trim()
    if (!token) return
    await runAction(
      `token:save:${hubId}`,
      async () => {
        await setSkillMarketHubToken(hubId, token)
        setTokenDrafts((current) => ({ ...current, [hubId]: "" }))
        const status = await loadSkillMarketHubTokenStatus(hubId)
        setTokenStatuses((current) => ({
          ...current,
          [hubId]: status.masked || (status.hasToken ? "••••" : ""),
        }))
        return t("settings.skillsDockExtensions.tokenSaved")
      },
      false,
    )
  }

  async function handleTokenClear(hubId: string) {
    await runAction(
      `token:clear:${hubId}`,
      async () => {
        await clearSkillMarketHubToken(hubId)
        setTokenStatuses((current) => ({ ...current, [hubId]: "" }))
        return t("settings.skillsDockExtensions.tokenCleared")
      },
      false,
    )
  }

  async function handleZipDryRun() {
    if (!zipPath.trim()) return
    setZipError(null)
    await runAction(
      "zip:dryRun",
      async () => {
        const report = await dryRunImportSkillZip(zipPath.trim())
        setZipReport(report)
      },
      false,
    ).catch((error) => setZipError(error instanceof Error ? error.message : String(error)))
  }

  async function handleZipImport(renamed = false) {
    if (!zipPath.trim()) return
    await runAction("zip:import", async () => {
      const report = renamed
        ? await importSkillZipRenamed(zipPath.trim())
        : await importSkillZip(zipPath.trim())
      onImported()
      return t("settings.skillsDockExtensions.importResult", {
        name: report.imported[0] ?? (zipRename.trim() || zipPath.trim()),
      })
    })
  }

  async function handleZipExport() {
    if (!exportSkillName.trim() || !exportPath.trim()) return
    await runAction("zip:export", async () => {
      const report = await exportSkillZip(exportSkillName.trim(), exportPath.trim())
      return t("settings.skillsDockExtensions.exportResult", { path: report.outputPath })
    })
  }

  async function handlePublishDraft() {
    if (!publishSkillName.trim() || !publishHubId) return
    await runAction(
      "publish:draft",
      async () => {
        const draft = await createSkillPublishDraft({
          skillName: publishSkillName.trim(),
          hubId: publishHubId,
        })
        setPublishDraft(draft)
        return draft.publishable
          ? t("settings.skillsDockExtensions.publishDraftReady", { name: draft.skillName })
          : (draft.error ?? t("settings.skillsDockExtensions.publishBlocked"))
      },
      false,
    )
  }

  async function handlePublishPush() {
    if (!publishDraft?.publishable) return
    const confirmed = window.confirm(
      t("settings.skillsDockExtensions.publishConfirm", { name: publishDraft.skillName }),
    )
    if (!confirmed) return
    await runAction("publish:push", async () => {
      const result = await pushSkillToMarketHub({
        skillName: publishDraft.skillName,
        hubId: publishDraft.hubId,
        confirmed: true,
      })
      if (!result.ok) throw new Error(result.error ?? result.status)
      return t("settings.skillsDockExtensions.publishResult", {
        name: result.skillName,
        source: result.sourceId,
      })
    })
  }

  return (
    <div className="flex-1 min-h-0 overflow-y-auto bg-gradient-to-b from-secondary/20 via-transparent to-transparent p-6">
      {!lockedSection && (
        <DockSectionNav activeSection={activeSection} onSectionChange={setActiveSection} />
      )}

      {visibleSection === "overview" && renderOverview()}
      {visibleSection === "local" &&
        (localSkillsContent ?? (
          <section className="mt-5 rounded-2xl border border-dashed border-border bg-card/80 p-6 text-sm text-muted-foreground">
            {t("settings.skillsDockExtensions.localSkillsEmpty")}
          </section>
        ))}
      {visibleSection === "market" && renderMarket()}
      {visibleSection === "io" && renderImportExport()}
      {visibleSection === "usage" && renderUsage()}
      {visibleSection === "settings" && renderSettings()}
    </div>
  )

  function renderOverview() {
    return (
      <div className="space-y-5 bg-[#f5f5f7] p-5 text-[#1d1d1f] dark:bg-background dark:text-foreground">
        <header>
          <h2 className="text-xl font-semibold">
            {t("settings.skillsDockExtensions.overviewTitle")}
          </h2>
          <p className="mt-1 text-[13px] text-[#888]">
            {t("settings.skillsDockExtensions.overviewSubtitle")}
          </p>
        </header>
        <OverviewStats stats={stats} appInstallCounts={externalAppInstallCounts} t={t} />
        <div className="grid gap-3 xl:grid-cols-3">
          <ScanStatusCard
            t={t}
            generatedAtLabel={generatedAtLabel}
            stats={stats}
            activeUsageRows={activeUsageRows.length}
            verificationLabel={verificationLabel}
            failedInstallCount={failedInstallCount}
            busy={actionBusy !== null}
            onRefresh={() => {
              onRefresh()
              void handleScanUsage()
            }}
          />
          <AppInstallBarCard counts={externalAppInstallCounts} t={t} />
          <TrendCard points={usageTrendPoints} max={maxTrendCount} t={t} />
        </div>
        <div className="grid gap-3 xl:grid-cols-3">
          <TopSkillsCard
            rows={topFiveUsageRows}
            skills={skills}
            t={t}
            onViewAll={() => setActiveSection("local")}
          />
          <TimelineCard rows={recentTimelineRows} t={t} onRefresh={() => void handleScanUsage()} />
          <QuickActionsCard
            t={t}
            onQuickImport={onQuickImport}
            onExport={() => setActiveSection("io")}
            onAddDir={onAddDir}
            onLogs={() => void handleScanUsage()}
          />
        </div>
        <OverviewFooter
          t={t}
          stats={stats}
          installed={installedAnywhereCount}
          notInstalled={notInstalledAnywhereCount}
          failed={failedInstallCount}
        />
      </div>
    )
  }

  function renderUsage() {
    const totalUsage = filteredUsageRows.reduce((total, row) => total + row.usageCount, 0)
    const todayKey = new Date().toISOString().slice(0, 10)
    const todayUsage = filteredRecentUsageRows
      .filter((row) => row.activatedAt.slice(0, 10) === todayKey)
      .reduce((total, row) => total + row.count, 0)
    const activeSkillNames = new Set(
      filteredRecentUsageRows
        .filter((row) => daysBetween(row.activatedAt, new Date()) <= 7)
        .map((row) => row.skillName),
    )
    const logStatus = rawUsageAppRows.length
      ? t("settings.skillsDockExtensions.usageStatusReady")
      : t("settings.skillsDockExtensions.usageStatusPending")
    const logStatusText = rawUsageAppRows.length
      ? t("settings.skillsDockExtensions.usageStatusReadyDesc")
      : t("settings.skillsDockExtensions.usageStatusPendingDesc")
    const sourceOptions = Array.from(
      new Set(["hope", ...rawUsageAppRows.map((row) => row.app), ...snapshotApps.map((row) => row.app)]),
    )
    const trendSeries = buildUsageTrendSeries(filteredUsageTrendRows)
    const appDistribution = buildUsageAppDistribution(filteredUsageAppRows, snapshotApps)

    return (
      <div className="space-y-4 bg-[#f5f5f7] p-5 text-[#1d1d1f] dark:bg-background dark:text-foreground">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap items-center gap-2.5">
              <span className="rounded-md border border-[#e5e7eb] bg-white px-3 py-1.5 text-[13px] text-[#555] dark:bg-card dark:text-muted-foreground">
              📅 {t("settings.skillsDockExtensions.last7Days")}
            </span>
            <Select value={usageAppFilter} onValueChange={setUsageAppFilter}>
              <SelectTrigger className="h-8 w-32 rounded-md border-[#e5e7eb] bg-white text-[13px] shadow-none dark:bg-card">
                <SelectValue placeholder={t("settings.skillsDockExtensions.usageAppFilterPlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("settings.skillsDockExtensions.allSources")}</SelectItem>
                {sourceOptions.map((app) => (
                  <SelectItem key={app} value={app}>
                    {appLabel(app as AppKind)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Select value={usageSkillFilter} onValueChange={setUsageSkillFilter}>
              <SelectTrigger className="h-8 w-40 rounded-md border-[#e5e7eb] bg-white text-[13px] shadow-none dark:bg-card">
                <SelectValue placeholder={t("settings.skillsDockExtensions.usageSkillFilterPlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("settings.skillsDockExtensions.allSkills")}</SelectItem>
                {usageDashboardRows.map((row) => (
                  <SelectItem key={row.skillName} value={row.skillName}>
                    {row.skillName}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Select
              value={usageGranularity}
              onValueChange={(value) => setUsageGranularity(value as "day" | "week")}
            >
              <SelectTrigger className="h-8 w-28 rounded-md border-[#e5e7eb] bg-white text-[13px] shadow-none dark:bg-card">
                <SelectValue placeholder={t("settings.skillsDockExtensions.usageGranularityPlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="day">{t("settings.skillsDockExtensions.perDay")}</SelectItem>
                <SelectItem value="week">{t("settings.skillsDockExtensions.perWeek")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <Button
            variant="outline"
            size="sm"
            className="h-8 rounded-md border-[#e5e7eb] bg-white px-3.5 text-[13px] shadow-none dark:bg-card"
            disabled={actionBusy === "scanUsage"}
            onClick={() => void handleScanUsage()}
          >
            <RotateCw className="mr-1.5 h-3.5 w-3.5" />
            {t("settings.skillsDockExtensions.refreshData")}
          </Button>
        </div>

        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          <UsageMetricCard icon="📈" iconClassName="bg-[#dbeafe] text-[#2563eb]" title={t("settings.skillsDockExtensions.totalUsageTitle")} value={formatNumber(totalUsage)} delta={t("settings.skillsDockExtensions.totalUsageDelta", { count: rawUsageAppRows.length || 1 })} />
          <UsageMetricCard icon="📅" iconClassName="bg-[#dcfce7] text-[#16a34a]" title={t("settings.skillsDockExtensions.todayUsageTitle")} value={formatNumber(todayUsage)} delta={t("settings.skillsDockExtensions.todayUsageDelta")} />
          <UsageMetricCard icon="📦" iconClassName="bg-[#f3e8ff] text-[#9333ea]" title={t("settings.skillsDockExtensions.activeSkillTitle")} value={activeSkillNames.size} delta={t("settings.skillsDockExtensions.activeSkillDelta", { count: filteredUsageRows.length })} />
          <UsageMetricCard icon="🗄️" iconClassName="bg-[#fef3c7] text-[#d97706]" title={t("settings.skillsDockExtensions.logSourceStatusTitle")} value={logStatus} delta={logStatusText} />
        </div>

        <div className="grid gap-3 xl:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)_minmax(0,1fr)]">
          <UsageTrendLinesCard series={trendSeries} granularity={usageGranularity} />
          <UsageAppDistributionCard rows={appDistribution} />
          <TopSkillsCard
            rows={topFiveUsageRows}
            skills={skills}
            t={t}
            onViewAll={() => setActiveSection("local")}
          />
        </div>

        <UsageRecentRecordsTable rows={filteredRecentUsageRows.slice(0, 8)} />
      </div>
    )
  }

  function renderMarket() {
    const normalizedSearch = marketSearch.trim().toLowerCase()
    const sourceOptions = Array.from(
      new Set([
        ...marketEntries.map((entry) => entry.sourceName),
        ...registryEntries.map((entry) => entry.sourceId),
      ]),
    ).filter(Boolean)
    const marketItems: MarketDisplayItem[] = [
      ...marketEntries.map((entry, index) => toRemoteMarketDisplayItem(entry, index)),
      ...registryEntries.map((entry, index) => toRegistryMarketDisplayItem(entry, index)),
    ]
    const visibleMarketItems = marketItems
      .filter((item) => {
        if (marketSourceFilter !== "all" && item.sourceName !== marketSourceFilter) return false
        if (!normalizedSearch) return true
        return [
          item.name,
          item.author,
          item.sourceName,
          item.version,
          item.description,
          item.category,
        ]
          .filter(Boolean)
          .join(" ")
          .toLowerCase()
          .includes(normalizedSearch)
      })
      .sort((left, right) => {
        if (marketSort === "updates")
          return Number(right.updateAvailable) - Number(left.updateAvailable)
        if (marketSort === "installed") return Number(right.installed) - Number(left.installed)
        return right.rankScore - left.rankScore || left.name.localeCompare(right.name)
      })
    const recommendedItems = visibleMarketItems.slice(0, 6)
    const recentItems = visibleMarketItems.slice(0, 4)
    const previewItem = recommendedItems[0]
    const installedMarketCount = marketItems.filter((item) => item.installed).length
    const installableMarketCount = marketItems.filter((item) => !item.installed).length
    const updateCount = marketItems.filter((item) => item.updateAvailable).length
    const officialCount = marketItems.filter((item) =>
      /official|官方|claw|hope/i.test(item.sourceName),
    ).length
    const collectionCount = marketItems.length || marketEntries.length + registryEntries.length
    const categoryCounts = marketItems.reduce<Record<string, number>>((counts, item) => {
      counts[item.category] = (counts[item.category] ?? 0) + 1
      return counts
    }, {})
    const categoryItems: MarketCategoryItem[] = [
      { icon: "</>", label: t("settings.skillsDockExtensions.categoryDev"), count: categoryCounts["开发"] ?? 0, className: "bg-[#dbeafe] text-[#1e40af]" },
      { icon: "⚡", label: t("settings.skillsDockExtensions.categoryEfficiency"), count: categoryCounts["效率"] ?? 0, className: "bg-[#fef3c7] text-[#92400e]" },
      { icon: "📄", label: t("settings.skillsDockExtensions.categoryDocs"), count: categoryCounts["文档"] ?? 0, className: "bg-[#dcfce7] text-[#166534]" },
      { icon: "🧪", label: t("settings.skillsDockExtensions.categoryTest"), count: categoryCounts["测试"] ?? 0, className: "bg-[#fee2e2] text-[#991b1b]" },
      { icon: "🎨", label: t("settings.skillsDockExtensions.categoryDesign"), count: categoryCounts["设计"] ?? 0, className: "bg-[#f3e8ff] text-[#6b21a8]" },
      { icon: "🤖", label: t("settings.skillsDockExtensions.categoryAutomation"), count: categoryCounts["自动化"] ?? 0, className: "bg-[#e0f2fe] text-[#075985]" },
      { icon: "🗄️", label: t("settings.skillsDockExtensions.categoryData"), count: categoryCounts["数据"] ?? 0, className: "bg-[#ecfdf5] text-[#047857]" },
      { icon: "👥", label: t("settings.skillsDockExtensions.categoryTeam"), count: categoryCounts["团队协作"] ?? 0, className: "bg-[#f1f5f9] text-[#334155]" },
    ]

    return (
      <div className="space-y-4 bg-[#f5f5f7] p-5 text-[#1d1d1f] dark:bg-background dark:text-foreground">
        <header className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
          <div className="min-w-0 flex-1">
              <h2 className="mb-1 text-xl font-semibold leading-none text-[#1d1d1f] dark:text-foreground">
              {t("settings.skillsDockExtensions.marketTitle")}
            </h2>
            <p className="text-[13px] leading-5 text-[#888]">
              {t("settings.skillsDockExtensions.marketDesc")}
            </p>
            <div className="mt-3 flex flex-col gap-2 lg:flex-row lg:items-center lg:gap-2.5">
              <Input
                value={marketSearch}
                onChange={(event) => setMarketSearch(event.target.value)}
                placeholder={t("settings.skillsDockExtensions.marketSearchPlaceholder")}
                className="h-9 max-w-[360px] flex-1 rounded-lg border-[#e5e7eb] bg-white px-3 text-[13px] shadow-none placeholder:text-[#aaa] dark:bg-card"
              />
              <MarketFilterSelect
                value="all"
                onChange={() => undefined}
                label={t("settings.skillsDockExtensions.allCategories")}
                options={[t("settings.skillsDockExtensions.allCategories") ]}
              />
              <MarketFilterSelect
                value={marketSourceFilter}
                onChange={setMarketSourceFilter}
                label={t("settings.skillsDockExtensions.allSources")}
                options={[t("settings.skillsDockExtensions.allSources"), ...sourceOptions]}
                optionValues={["all", ...sourceOptions]}
              />
              <MarketFilterSelect
                value="all"
                onChange={() => undefined}
                label={t("settings.skillsDockExtensions.allPrices")}
                options={[t("settings.skillsDockExtensions.allPrices") ]}
              />
              <MarketFilterSelect
                value={marketSort}
                onChange={setMarketSort}
                label={t("settings.skillsDockExtensions.sortRecommended")}
                options={[t("settings.skillsDockExtensions.sortRecommended"), t("settings.skillsDockExtensions.sortUpdates"), t("settings.skillsDockExtensions.sortInstalled")]}
                optionValues={["recommended", "updates", "installed"]}
              />
            </div>
          </div>
          <button
            type="button"
            className="inline-flex h-9 shrink-0 items-center justify-center rounded-md bg-[#2563eb] px-4 text-[13px] font-medium text-white shadow-sm hover:bg-[#1d4ed8]"
            onClick={() => setActiveSection("io")}
          >
            ⬆️ {t("settings.skillsDockExtensions.publishSkill")}
          </button>
        </header>

        {marketError && (
          <div className="rounded-[10px] border border-[#fecaca] bg-[#fef2f2] p-3 text-xs text-[#b91c1c]">
            {marketError}
          </div>
        )}

        {!!market?.sources.length && (
          <section className="grid gap-2 md:grid-cols-2 xl:grid-cols-3">
            {market.sources.map((source) => {
              const isSecurityBlocked = source.error?.includes("SSRF policy Strict blocked")
              return (
                <div
                  key={source.id}
                  className={`rounded-[10px] border p-3 text-xs shadow-[0_1px_3px_rgba(0,0,0,0.04)] ${
                    source.status === "error"
                      ? "border-[#fecaca] bg-[#fef2f2] text-[#991b1b]"
                      : "border-[#dbeafe] bg-[#eff6ff] text-[#1e3a8a]"
                  }`}
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="truncate font-semibold">{source.name}</div>
                      <div className="mt-0.5 truncate opacity-80">{source.url}</div>
                    </div>
                    <span className="shrink-0 rounded-full bg-white/70 px-2 py-0.5 font-medium">
                      {source.status === "error"
                        ? t("settings.skillsDockExtensions.marketSourceStatusError")
                        : t("settings.skillsDockExtensions.marketSourceStatusReady")}
                    </span>
                  </div>
                  <div className="mt-2 flex flex-wrap gap-2 opacity-90">
                    <span>
                      {t("settings.skillsDockExtensions.marketSourceEntries", {
                        count: source.entryCount,
                      })}
                    </span>
                    <span>
                      {t("settings.skillsDockExtensions.marketSourceUpdates", {
                        count: source.updateCount,
                      })}
                    </span>
                  </div>
                  {source.error && (
                    <div className="mt-2 rounded bg-white/65 p-2 leading-5">
                      {isSecurityBlocked && (
                        <div className="mb-1 font-medium">
                          {t("settings.skillsDockExtensions.marketSourceSecurityBlockedHint")}
                        </div>
                      )}
                      <div className="break-words opacity-90">{source.error}</div>
                    </div>
                  )}
                </div>
              )
            })}
          </section>
        )}

        <div className="grid gap-4 xl:grid-cols-[2fr_1fr]">
          <main className="space-y-4">
            <section className="relative min-h-[164px] overflow-hidden rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
              <div className="absolute inset-0 bg-gradient-to-br from-[#eef2ff] via-[#eff6ff] to-[#faf5ff]" />
              <div className="absolute -right-8 -top-10 h-44 w-56 rounded-full bg-[#8b5cf6]/20 blur-2xl" />
              <div className="absolute right-6 top-6 hidden h-28 w-40 sm:block">
                <div className="absolute right-6 top-2 h-16 w-24 rotate-6 rounded-xl border border-white/70 bg-white/70 p-2 shadow-lg backdrop-blur">
                  <div className="mb-1 h-1.5 w-10 rounded bg-[#93c5fd]" />
                  <div className="mb-1 h-1.5 w-16 rounded bg-[#c4b5fd]" />
                  <div className="h-1.5 w-12 rounded bg-[#86efac]" />
                </div>
                <div className="absolute bottom-2 left-2 grid h-12 w-12 place-items-center rounded-2xl bg-[#2563eb] text-xl text-white shadow-lg">
                  ✓
                </div>
                <div className="absolute bottom-0 right-0 grid h-10 w-10 place-items-center rounded-xl bg-[#f59e0b] text-lg text-white shadow-lg">
                  ★
                </div>
                <div className="absolute left-10 top-0 text-2xl text-[#8b5cf6]">↗</div>
              </div>
              <div className="relative z-10 max-w-[66%] min-w-[260px]">
                <span className="inline-flex rounded bg-[#dbeafe] px-2 py-0.5 text-[11px] font-medium text-[#1e40af]">
                  {t("settings.skillsDockExtensions.weeklyFeatured")}
                </span>
                <h3 className="mt-2 text-lg font-semibold leading-6 text-[#1d1d1f] dark:text-foreground">
                  {t("settings.skillsDockExtensions.featuredTitle")}
                </h3>
                <p className="mt-1 text-[13px] leading-5 text-[#666]">
                  {t("settings.skillsDockExtensions.featuredDesc")}
                </p>
                <button
                  type="button"
                  className="mt-3 rounded-md bg-[#2563eb] px-4 py-2 text-[13px] font-medium text-white hover:bg-[#1d4ed8] disabled:opacity-60"
                  onClick={onRefreshMarket}
                  disabled={marketLoading}
                >
                  {t("settings.skillsDockExtensions.viewCollection")}
                </button>
              </div>
            </section>

            <div className="flex gap-2.5 overflow-x-auto pb-1">
              {categoryItems.map((item) => (
                <MarketCategoryCard key={item.label} item={item} />
              ))}
            </div>

            <section>
                <h3 className="mb-3 text-sm font-semibold text-[#1d1d1f] dark:text-foreground">
                {t("settings.skillsDockExtensions.recommendedSkills")}
              </h3>
              <div className="grid gap-3 lg:grid-cols-2">
                {recommendedItems.map((item, index) => (
                  <MarketSkillCard
                    key={item.id}
                    item={item}
                    index={index}
                    busy={actionBusy === item.busyKey}
                    onAction={() => void runMarketDisplayAction(item)}
                  />
                ))}
                {!recommendedItems.length && (
                  <div className="rounded-[10px] bg-white p-6 text-center text-xs text-[#888] shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
                    {t("settings.skillsDockExtensions.noMarketEntries")}
                  </div>
                )}
              </div>
              <button
                type="button"
                className="mt-3 text-xs font-medium text-[#2563eb]"
                onClick={onRefreshMarket}
              >
                {t("settings.skillsDockExtensions.viewMoreSkills")}
              </button>
            </section>

            <section>
                <h3 className="mb-3 text-sm font-semibold text-[#1d1d1f] dark:text-foreground">
                {t("settings.skillsDockExtensions.recentUpdates")}
              </h3>
              <div className="flex gap-2.5 overflow-x-auto pb-1">
                {recentItems.map((item, index) => (
                  <MarketUpdateCard key={item.id} item={item} index={index} />
                ))}
                {!recentItems.length && (
                  <div className="min-w-[200px] rounded-lg bg-white p-3 text-xs text-[#888] shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
                    {t("settings.skillsDockExtensions.noRecentUpdates")}
                  </div>
                )}
              </div>
            </section>
          </main>

          <aside className="space-y-4">
            <MarketStatsCard
              title={t("settings.skillsDockExtensions.marketStatsTitle")}
              items={[
                { icon: "📦", label: t("settings.skillsDockExtensions.collected"), value: collectionCount, unit: "Skills" },
                { icon: "🛡️", label: t("settings.skillsDockExtensions.officialVerified"), value: officialCount, unit: t("settings.skillsDockExtensions.unitCount") },
                {
                  icon: "📈",
                  label: t("settings.skillsDockExtensions.thisWeekNew"),
                  value:
                    market?.sources.reduce((total, source) => total + source.entryCount, 0) ?? 0,
                  unit: t("settings.skillsDockExtensions.unitCount"),
                },
              ]}
            />
            <MarketStatsCard
              title={t("settings.skillsDockExtensions.myDownloads")}
              items={[
                {
                  icon: "✅",
                  label: t("settings.skillsDockExtensions.installedFromMarket"),
                  value: installedMarketCount,
                  unit: t("settings.skillsDockExtensions.unitCount"),
                },
                { icon: "🔔", label: t("settings.skillsDockExtensions.pendingUpdate"), value: updateCount, unit: t("settings.skillsDockExtensions.unitCount") },
                {
                  icon: "📦",
                  label: t("settings.skillsDockExtensions.installable"),
                  value: installableMarketCount,
                  unit: t("settings.skillsDockExtensions.unitCount"),
                },
              ]}
            />
            <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
                <h3 className="mb-3 text-sm font-semibold text-[#1d1d1f] dark:text-foreground">
                {t("settings.skillsDockExtensions.topList")}
              </h3>
              <div className="space-y-2">
                {recommendedItems.slice(0, 5).map((item, index) => (
                  <MarketRankRow key={item.id} item={item} index={index} />
                ))}
                {!recommendedItems.length && (
                  <div className="text-xs text-[#888]">{t("settings.skillsDockExtensions.noTopList")}</div>
                )}
              </div>
              <button type="button" className="mt-3 text-xs font-medium text-[#2563eb]">
                {t("settings.skillsDockExtensions.viewMoreRankings")}
              </button>
            </section>
            {previewItem && (
              <MarketPreviewCard
                item={previewItem}
                busy={actionBusy === previewItem.busyKey}
                onAction={() => void runMarketDisplayAction(previewItem)}
              />
            )}
          </aside>
        </div>

        <footer className="flex flex-col justify-between gap-2 text-xs text-[#888] sm:flex-row">
            <span>
            {t("settings.skillsDockExtensions.marketSources")}
            {market?.sources.map((source) => source.name).join(" + ") || t("settings.skillsDockExtensions.marketSourcesFallback")}
          </span>
          <span>{t("settings.skillsDockExtensions.autoRefreshEvery10Min")}</span>
        </footer>
      </div>
    )
  }

  async function runMarketDisplayAction(item: MarketDisplayItem) {
    if (item.kind === "remote" && item.remoteEntry) {
      await handleMarketAction(item.remoteEntry)
      return
    }
    if (item.kind === "registry" && item.registryEntry) {
      await runAction(item.busyKey, async () => {
        const report = item.updateAvailable
          ? await updateRegistrySkill(item.registryEntry!.skillPath, item.registryEntry!.name)
          : await installRegistrySkill(item.registryEntry!.skillPath, item.registryEntry!.name)
        return item.updateAvailable
          ? t("settings.skillsDockExtensions.registryUpdateResult", { name: report.name })
          : t("settings.skillsDockExtensions.registryInstallResult", { name: report.name })
      })
    }
  }

  function renderImportExport() {
    const previewNames = zipReport?.skillNames.length
      ? zipReport.skillNames
      : zipPath.trim()
        ? [
            zipPath
              .split(/[\\/]/)
              .pop()
              ?.replace(/\.zip$/i, "") || zipPath,
          ]
        : []
    const previewRows = previewNames.map((name, index) => {
      const hasIssue = zipReport?.issues.some((issue) =>
        `${issue.code} ${issue.message}`.toLowerCase().includes(name.toLowerCase()),
      )
      return {
        name,
        status: hasIssue ? ("warning" as const) : ("ok" as const),
        target: `~/Skills/${zipRename.trim() || name}`,
        icon: ["💻", "🧪", "🚀", "👁️"][index % 4],
        apps: EXTERNAL_APPS.map((app) => ({
          app,
          installed: matrixRows
            .find((row) => row.skillName === name)
            ?.apps.some((state) => state.app === app && state.installed),
        })),
      }
    })
    const selectedSkill = skills.find((skill) => skill.name === exportSkillName) ?? skills[0]
    const estimatedFiles = Math.max(1, zipReport?.entryCount ?? 14)
    const estimatedSize = formatBytes(zipReport?.totalUncompressedSize ?? 0)
    const recentRows = [
      zipReport && {
        type: "import" as const,
        file: zipReport.path.split(/[\\/]/).pop() || zipReport.path,
        content: `${zipReport.skillCount} Skills`,
        time: t("settings.skillsDockExtensions.justNow"),
        status: zipReport.ok ? ("success" as const) : ("warning" as const),
      },
      exportPath.trim() && {
        type: "export" as const,
        file: exportPath.split(/[\\/]/).pop() || exportPath,
        content: `1 Skills / ${estimatedSize}`,
        time: t("settings.skillsDockExtensions.pending"),
        status: "success" as const,
      },
    ].filter(Boolean) as Array<{
      type: "import" | "export"
      file: string
      content: string
      time: string
      status: "success" | "warning"
    }>

    return (
      <div className="space-y-4 bg-[#f5f5f7] p-5 text-[#1d1d1f] dark:bg-background dark:text-foreground">
        <header className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <h2 className="text-xl font-semibold">导入/导出</h2>
            <p className="mt-1 text-[13px] text-[#888]">
              通过 ZIP 快速导入或导出 Skills，支持迁移、备份与发布。
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <SettingTopButton onClick={onQuickImport}>📁 选择 ZIP</SettingTopButton>
            <SettingTopButton
              onClick={() => setExportPath(exportPath || "~/Downloads/skill-dock-export.zip")}
            >
              📁 选择导出路径
            </SettingTopButton>
          </div>
        </header>

        <div className="grid gap-4 xl:grid-cols-[1.4fr_1fr]">
          <div className="space-y-4">
            <section className="rounded-[10px] bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
              <h3 className="text-sm font-semibold">从 ZIP 导入 Skills</h3>
              <button
                type="button"
                className="mt-4 flex w-full flex-col items-center justify-center rounded-lg border border-dashed border-[#d1d5db] bg-[#fafafa] px-4 py-10 text-center"
                onClick={onQuickImport}
              >
                <Upload className="h-7 w-7 text-[#9ca3af]" />
                <span className="mt-3 text-[13px] font-medium text-[#374151]">
                  将 ZIP 文件拖拽到此处，或点击选择文件
                </span>
                <span className="mt-1 text-xs text-[#888]">
                  支持包含一个或多个 Skill 的 ZIP 压缩包
                </span>
              </button>
              <div className="mt-4 grid gap-2 md:grid-cols-[minmax(0,1fr)_minmax(0,0.7fr)_auto]">
                <Input
                  value={zipPath}
                  onChange={(event) => setZipPath(event.target.value)}
                  placeholder={t("settings.skillsDockExtensions.zipPathPlaceholder")}
                  className="h-9 border-[#e5e7eb] bg-white text-[13px]"
                />
                <Input
                  value={zipRename}
                  onChange={(event) => setZipRename(event.target.value)}
                  placeholder={t("settings.skillsDockExtensions.renamePlaceholder")}
                  className="h-9 border-[#e5e7eb] bg-white text-[13px]"
                />
                <Button
                  variant="secondary"
                  className="h-9 gap-1.5 border-[#e5e7eb] bg-white text-[13px] text-[#374151]"
                  onClick={onQuickImport}
                >
                  <FolderOpen className="h-3.5 w-3.5" />
                  选择 ZIP 文件
                </Button>
              </div>
            </section>

            <section className="rounded-[10px] bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
              <h3 className="text-sm font-semibold">导入选项</h3>
              <div className="mt-3 space-y-3">
                <ImportOption
                  title="导入到本地目录"
                  desc="将技能导入到 Hope Agent 本地 Skills 根目录"
                />
                <ImportOption title="自动扫描" desc="导入完成后自动重新扫描并刷新列表" />
                <ImportOption
                  title="若同名则提示覆盖"
                  desc="遇到同名 Skill 时沿用后端校验与确认流程"
                />
              </div>
            </section>

            <section className="rounded-[10px] bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <h3 className="text-sm font-semibold">
                  导入预览
                  <span className="ml-1 font-normal text-[#888]">
                    （ZIP 中发现 {zipReport?.skillCount ?? previewRows.length} 个 Skills）
                  </span>
                </h3>
                <Button
                  variant="secondary"
                  className="h-8 border-[#e5e7eb] bg-white text-xs text-[#374151]"
                  onClick={() => void handleZipDryRun()}
                  disabled={!zipPath.trim() || actionBusy !== null}
                >
                  预检查
                </Button>
              </div>
              {zipError && (
                <div className="mt-3 rounded-lg border border-[#fecaca] bg-[#fef2f2] p-3 text-xs text-[#b91c1c]">
                  {zipError}
                </div>
              )}
              <div className="mt-3 overflow-x-auto">
                <table className="w-full min-w-[720px] text-left text-[13px]">
                  <thead className="border-b border-[#f0f0f0] text-[11px] font-medium text-[#888]">
                    <tr>
                      <th className="py-2 pr-3">Skill 名称</th>
                      <th className="py-2 pr-3">验证状态</th>
                      <th className="py-2 pr-3">目标目录</th>
                      {EXTERNAL_APPS.map((app) => (
                        <th key={app} className="py-2 pr-3 text-center">
                          安装到 {t(`settings.skillsDockExtensions.app.${app}`)}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {previewRows.length ? (
                      previewRows.map((row) => (
                        <tr key={row.name} className="border-b border-[#f0f0f0] last:border-0">
                          <td className="py-3 pr-3">
                            <span className="inline-flex items-center gap-2 font-medium">
                              <span>{row.icon}</span>
                              <span>{row.name}</span>
                            </span>
                          </td>
                          <td className="py-3 pr-3">
                            <ImportStatus status={row.status} />
                          </td>
                          <td className="py-3 pr-3 text-[#666]">{row.target}</td>
                          {row.apps.map(({ app, installed }) => (
                            <td key={app} className="py-3 pr-3 text-center">
                              <InstallMark installed={Boolean(installed)} />
                            </td>
                          ))}
                        </tr>
                      ))
                    ) : (
                      <tr>
                        <td colSpan={7} className="py-8 text-center text-xs text-[#888]">
                          输入 ZIP 路径并点击预检查后展示真实导入预览。
                        </td>
                      </tr>
                    )}
                  </tbody>
                </table>
              </div>
              <div className="mt-4 flex flex-col gap-3 border-t border-[#f0f0f0] pt-3 lg:flex-row lg:items-center lg:justify-between">
                <p className="text-xs text-[#888]">提示：带“警告”的项目导入后可手动核查并修复。</p>
                <span className="text-xs text-[#888]">
                  {zipReport?.skillCount ?? previewRows.length} 个 Skills 将被导入
                </span>
              </div>
              <div className="mt-4 flex justify-end gap-2">
                <Button
                  variant="secondary"
                  className="h-9 border-[#e5e7eb] bg-white px-5 text-[13px] text-[#374151]"
                  onClick={() => {
                    setZipReport(null)
                    setZipError(null)
                  }}
                >
                  取消
                </Button>
                <Button
                  className="h-9 bg-[#2563eb] px-6 text-[13px] text-white"
                  onClick={() => void handleZipImport(Boolean(zipRename.trim()))}
                  disabled={!zipPath.trim() || actionBusy !== null}
                >
                  导入并安装
                </Button>
              </div>
            </section>
          </div>

          <div className="space-y-4">
            <section className="rounded-[10px] bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
              <div className="flex items-center justify-between gap-3">
                <h3 className="text-sm font-semibold">导出所选 Skills</h3>
                <button
                  type="button"
                  className="text-xs font-medium text-[#2563eb]"
                  onClick={() => setExportSkillName(skills[0]?.name ?? "")}
                >
                  清空选择
                </button>
              </div>
              <div className="mt-3 text-[13px] text-[#1d1d1f]">
                已选择 {selectedSkill ? 1 : 0} 个 Skills
              </div>
              <div className="mt-2 space-y-1">
                {selectedSkill ? (
                  <div className="flex items-center gap-3 border-b border-[#f0f0f0] py-2.5">
                    <span className="flex h-4 w-4 items-center justify-center rounded-[3px] bg-[#2563eb] text-[10px] text-white">
                      ✓
                    </span>
                    <span className="text-lg">{selectedSkill.display?.emoji || "💻"}</span>
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-[13px] font-medium">{selectedSkill.name}</div>
                      <div className="truncate text-xs text-[#888]">
                        {selectedSkill.source === "bundled" ? "bundled" : "local"}
                      </div>
                    </div>
                    <Select value={exportSkillName} onValueChange={setExportSkillName}>
                      <SelectTrigger className="h-8 w-32 border-[#e5e7eb] bg-white text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {skills.map((item) => (
                          <SelectItem key={item.name} value={item.name}>
                            {item.name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                ) : (
                  <div className="py-6 text-center text-xs text-[#888]">暂无可导出的 Skill。</div>
                )}
              </div>
            </section>

            <section className="rounded-[10px] bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
              <div className="grid gap-4 sm:grid-cols-2">
                <div>
                  <h4 className="mb-2 text-[13px] font-semibold">导出内容摘要</h4>
                  <ExportSummaryRow label="Skills 数量" value={selectedSkill ? 1 : 0} />
                  <ExportSummaryRow label="预计文件数" value={estimatedFiles} />
                  <ExportSummaryRow label="预计大小" value={estimatedSize} />
                </div>
                <div>
                  <h4 className="mb-2 text-[13px] font-semibold">将包含以下内容</h4>
                  {[
                    "SKILL.md（技能描述）",
                    "assets/（资源文件）",
                    "schema.json（Schema）",
                    "examples/（示例与用例）",
                    "其他配置文件（如有）",
                  ].map((item) => (
                    <div key={item} className="flex items-center gap-2 py-0.5 text-xs text-[#555]">
                      <span className="text-[#22c55e]">✓</span>
                      <span>{item}</span>
                    </div>
                  ))}
                </div>
              </div>
              <div className="mt-4 border-t border-[#f0f0f0] pt-4">
                <div className="text-[13px] font-semibold">压缩选项</div>
                <div className="mt-2 flex flex-wrap gap-5 text-[13px] text-[#374151]">
                  <span className="inline-flex items-center gap-2">
                    <span className="h-3.5 w-3.5 rounded-full border-[4px] border-[#2563eb]" />
                    标准压缩<span className="text-xs text-[#888]">（推荐）</span>
                  </span>
                  <span className="inline-flex items-center gap-2">
                    <span className="h-3.5 w-3.5 rounded-full border border-[#d1d5db]" />
                    最快速度<span className="text-xs text-[#888]">（体积更大）</span>
                  </span>
                </div>
              </div>
              <div className="mt-4">
                <label className="mb-1.5 block text-[13px] font-medium">导出到</label>
                <div className="flex gap-2">
                  <Input
                    value={exportPath}
                    onChange={(event) => setExportPath(event.target.value)}
                    placeholder={t("settings.skillsDockExtensions.exportPathPlaceholder")}
                    className="h-9 flex-1 border-[#e5e7eb] bg-white text-[13px] text-[#666]"
                  />
                  <button
                    type="button"
                    className="rounded-md border border-[#e5e7eb] bg-white px-3 text-[#374151]"
                    onClick={() => setExportPath(exportPath || "~/Downloads/skill-dock-export.zip")}
                  >
                    <FolderOpen className="h-4 w-4" />
                  </button>
                </div>
              </div>
              <Button
                className="mt-4 h-10 w-full bg-[#2563eb] text-sm font-medium text-white"
                onClick={() => void handleZipExport()}
                disabled={!exportSkillName || !exportPath.trim() || actionBusy !== null}
              >
                导出为 ZIP
              </Button>
            </section>

            <section className="rounded-[10px] bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
              <h3 className="text-sm font-semibold">发布到 Hub</h3>
              <div className="mt-3 grid gap-2">
                <Select value={publishSkillName} onValueChange={setPublishSkillName}>
                  <SelectTrigger className="h-9 border-[#e5e7eb] bg-white text-[13px]">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {skills.map((skill) => (
                      <SelectItem key={skill.name} value={skill.name}>
                        {skill.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Select value={publishHubId} onValueChange={setPublishHubId}>
                  <SelectTrigger className="h-9 border-[#e5e7eb] bg-white text-[13px]">
                    <SelectValue placeholder={t("settings.skillsDockExtensions.selectHub")} />
                  </SelectTrigger>
                  <SelectContent>
                    {writableHubs.map((hub) => (
                      <SelectItem key={hub.id} value={hub.id}>
                        {hub.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <div className="flex gap-2">
                  <Button
                    variant="secondary"
                    className="h-9 flex-1 border-[#e5e7eb] bg-white text-[13px] text-[#374151]"
                    onClick={() => void handlePublishDraft()}
                    disabled={!publishSkillName || !publishHubId || actionBusy !== null}
                  >
                    {t("settings.skillsDockExtensions.createDraft")}
                  </Button>
                  <Button
                    className="h-9 flex-1 bg-[#2563eb] text-[13px] text-white"
                    onClick={() => void handlePublishPush()}
                    disabled={!publishDraft?.publishable || actionBusy !== null}
                  >
                    {t("settings.skillsDockExtensions.pushPublish")}
                  </Button>
                </div>
                {publishDraft && (
                  <div className="rounded-lg border border-[#e5e7eb] bg-[#fafafa] p-3 text-xs text-[#666]">
                    {publishDraft.publishable
                      ? t("settings.skillsDockExtensions.publishDraftReady", {
                          name: publishDraft.skillName,
                        })
                      : publishDraft.error || t("settings.skillsDockExtensions.publishBlocked")}
                  </div>
                )}
              </div>
            </section>

            <section className="rounded-[10px] bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
              <h3 className="mb-3 text-sm font-semibold">最近导入/导出记录</h3>
              <div className="space-y-1">
                {recentRows.length ? (
                  recentRows.map((row) => <RecentIoRow key={`${row.type}:${row.file}`} row={row} />)
                ) : (
                  <div className="py-6 text-center text-xs text-[#888]">
                    暂无本次会话的导入/导出记录。
                  </div>
                )}
              </div>
              <button type="button" className="mt-3 text-xs font-medium text-[#2563eb]">
                查看全部记录 →
              </button>
            </section>
          </div>
        </div>
      </div>
    )
  }

  function renderSettings() {
    return (
      <div className="space-y-4 bg-[#f5f5f7] p-5 text-[#1d1d1f] dark:bg-background dark:text-foreground">
        <header className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <h2 className="text-xl font-semibold">
              {t("settings.skillsDockExtensions.settingsPageTitle")}
            </h2>
            <p className="mt-1 text-[13px] text-[#888]">
              {t("settings.skillsDockExtensions.settingsPageSubtitle")}
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <SettingTopButton
              onClick={() => {
                onRefresh()
                void handleScanUsage()
              }}
            >
              ⟳ {t("settings.skillsDockExtensions.rescanNow")}
            </SettingTopButton>
            <SettingTopButton onClick={() => void refreshHubConfig()}>
              {t("settings.skillsDockExtensions.restoreDefault")}
            </SettingTopButton>
            <button
              type="button"
              className="rounded-md bg-[#2563eb] px-4 py-1.5 text-[13px] text-white"
              onClick={() => void refreshHubConfig()}
            >
              {t("settings.skillsDockExtensions.saveSettings")}
            </button>
          </div>
        </header>
        {hubError && (
          <div className="rounded-[10px] border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive">
            {hubError}
          </div>
        )}
        <div className="grid gap-4 xl:grid-cols-[1.2fr_1fr]">
          <div className="space-y-4">
            <SettingCard
              title={t("settings.skillsDockExtensions.defaultScanDirsTitle")}
              desc={t("settings.skillsDockExtensions.defaultScanDirsDesc")}
            >
              <div className="space-y-2">
                {settingsAppRows.map((row) => (
                  <SettingAppDirRow key={row.app} row={row} t={t} />
                ))}
              </div>
            </SettingCard>
            <SettingCard
              title={t("settings.skillsDockExtensions.customSkillDirsTitle")}
              desc={t("settings.skillsDockExtensions.customSkillDirsDesc")}
              action={
                <SettingButton onClick={onAddDir}>
                  + {t("settings.skillsDockExtensions.addDir")}
                </SettingButton>
              }
            >
              <div className="flex items-center justify-between border-b border-[#e5e7eb] pb-2 text-xs text-[#888]">
                <span>{t("settings.skillsDockExtensions.dirPath")}</span>
                <span>{t("settings.skillsDockExtensions.actions")}</span>
              </div>
              <div className="divide-y divide-[#f0f0f0]">
                {(customSkillDirs.length
                  ? customSkillDirs
                  : [t("settings.skillsDockExtensions.noCustomDirs")]
                ).map((dir, index) => (
                  <div
                    key={`${dir}:${index}`}
                    className="flex items-center justify-between gap-3 py-3 text-[13px]"
                  >
                    <span className="min-w-0 truncate" title={dir}>
                      {shortPath(dir)}
                    </span>
                    <span className="flex gap-2 text-[#9ca3af]">
                      <Pencil className="h-3.5 w-3.5 opacity-40" />
                      <Trash2 className="h-3.5 w-3.5 opacity-40" />
                    </span>
                  </div>
                ))}
              </div>
            </SettingCard>
            <SettingCard
              title={t("settings.skillsDockExtensions.autoScanTitle")}
              desc={t("settings.skillsDockExtensions.autoScanDesc")}
            >
              <SettingToggleRow
                title={t("settings.skillsDockExtensions.scanOnStartup")}
                desc={t("settings.skillsDockExtensions.scanOnStartupDesc")}
                checked
              />
              <SettingToggleRow
                title={t("settings.skillsDockExtensions.watchFileChanges")}
                desc={t("settings.skillsDockExtensions.watchFileChangesDesc")}
                checked
              />
              <SettingSelectRow
                title={t("settings.skillsDockExtensions.scheduledScan")}
                desc={t("settings.skillsDockExtensions.scheduledScanDesc")}
                value={t("settings.skillsDockExtensions.every30Minutes")}
              />
            </SettingCard>
            <SettingCard
              title={t("settings.skillsDockExtensions.marketSourcesTitle")}
              desc={t("settings.skillsDockExtensions.marketSourcesDesc")}
            >
              <div className="flex gap-2">
                <Input
                  value={marketSourceDraft}
                  onChange={(event) => setMarketSourceDraft(event.target.value)}
                  placeholder={t("settings.skillsDockExtensions.marketSourcePlaceholder")}
                  className="h-8 text-xs"
                />
                <SettingButton
                  disabled={!marketSourceDraft.trim() || marketSourceUrls.length >= 5}
                  onClick={handleAddMarketSource}
                >
                  {t("settings.skillsDockExtensions.addMarketSource")}
                </SettingButton>
              </div>
              <div className="mt-3 flex flex-wrap gap-2 text-[11px] text-[#666]">
                <span className="rounded bg-[#f5f5f7] px-2 py-1">
                  {t("settings.skillsDockExtensions.defaultMarketSource")}
                </span>
                {marketSourceUrls.map((url) => (
                  <button
                    key={url}
                    type="button"
                    className="max-w-full truncate rounded bg-[#f5f5f7] px-2 py-1"
                    onClick={() => handleRemoveMarketSource(url)}
                  >
                    {url}
                  </button>
                ))}
              </div>
            </SettingCard>
          </div>
          <div className="space-y-4">
            <SettingCard
              title={t("settings.skillsDockExtensions.logParsingTitle")}
              desc={t("settings.skillsDockExtensions.logParsingDesc")}
            >
              <div className="grid grid-cols-[5rem_minmax(0,1fr)_3.5rem] gap-2 border-b border-[#e5e7eb] pb-2 text-xs text-[#888]">
                <span>{t("settings.skillsDockExtensions.appColumn")}</span>
                <span>{t("settings.skillsDockExtensions.logPath")}</span>
                <span className="text-right">{t("settings.skillsDockExtensions.appParsing")}</span>
              </div>
              <div className="space-y-2 pt-2">
                {settingsAppRows.map((row) => (
                  <SettingLogRow key={row.app} row={row} t={t} />
                ))}
              </div>
            </SettingCard>
            <SettingCard
              title={t("settings.skillsDockExtensions.validationPrefsTitle")}
              desc={t("settings.skillsDockExtensions.validationPrefsDesc")}
            >
              <SettingToggleRow
                title={t("settings.skillsDockExtensions.strictValidation")}
                desc={t("settings.skillsDockExtensions.strictValidationDesc")}
                checked
              />
              <SettingToggleRow
                title={t("settings.skillsDockExtensions.validateOnImport")}
                desc={t("settings.skillsDockExtensions.validateOnImportDesc")}
                checked
              />
              <SettingToggleRow
                title={t("settings.skillsDockExtensions.showContentDiff")}
                desc={t("settings.skillsDockExtensions.showContentDiffDesc")}
                checked
              />
            </SettingCard>
            <SettingCard
              title={t("settings.skillsDockExtensions.otherSettingsTitle")}
              desc={t("settings.skillsDockExtensions.otherSettingsDesc")}
            >
              <SettingToggleRow
                title={t("settings.skillsDockExtensions.scanNotifications")}
                desc={t("settings.skillsDockExtensions.scanNotificationsDesc")}
                checked={false}
              />
              <SettingSelectRow
                title={t("settings.skillsDockExtensions.keepScanHistory")}
                desc={t("settings.skillsDockExtensions.keepScanHistoryDesc")}
                value={t("settings.skillsDockExtensions.keep7Days")}
              />
            </SettingCard>
            <SettingCard
              title={t("settings.skillsDockExtensions.marketManagementTitle")}
              desc={t("settings.skillsDockExtensions.marketManagementDesc")}
              action={
                <SettingButton disabled={hubLoading} onClick={() => void refreshHubConfig()}>
                  {t("settings.skillsDockExtensions.refresh")}
                </SettingButton>
              }
            >
              <div className="space-y-3">
                {hubs.map((hub) => (
                  <HubTokenCard
                    key={hub.id}
                    hub={hub}
                    tokenDraft={tokenDrafts[hub.id] ?? ""}
                    tokenStatus={tokenStatuses[hub.id] ?? ""}
                    busy={actionBusy !== null}
                    t={t}
                    onDraft={(value) =>
                      setTokenDrafts((current) => ({ ...current, [hub.id]: value }))
                    }
                    onSave={() => void handleTokenSave(hub.id)}
                    onClear={() => void handleTokenClear(hub.id)}
                    onToggle={() =>
                      void runAction(
                        `hub:enabled:${hub.id}`,
                        async () => {
                          const config = await setSkillMarketHubEnabled(hub.id, !hub.enabled)
                          setHubConfig(config)
                          return hub.enabled
                            ? t("settings.skillsDockExtensions.hubDisabled")
                            : t("settings.skillsDockExtensions.hubEnabled")
                        },
                        false,
                      )
                    }
                  />
                ))}
                <div className="grid gap-2 md:grid-cols-2">
                  <Input
                    value={hubForm.id}
                    onChange={(event) =>
                      setHubForm((current) => ({ ...current, id: event.target.value }))
                    }
                    placeholder={t("settings.skillsDockExtensions.hubIdPlaceholder")}
                    className="h-8 text-xs"
                  />
                  <Input
                    value={hubForm.name}
                    onChange={(event) =>
                      setHubForm((current) => ({ ...current, name: event.target.value }))
                    }
                    placeholder={t("settings.skillsDockExtensions.hubNamePlaceholder")}
                    className="h-8 text-xs"
                  />
                  <Input
                    value={hubForm.baseUrl}
                    onChange={(event) =>
                      setHubForm((current) => ({ ...current, baseUrl: event.target.value }))
                    }
                    placeholder={t("settings.skillsDockExtensions.hubUrlPlaceholder")}
                    className="h-8 text-xs md:col-span-2"
                  />
                  <Button
                    size="sm"
                    className="h-8 bg-[#2563eb] text-xs text-white md:col-span-2"
                    disabled={
                      !hubForm.name.trim() || !hubForm.baseUrl.trim() || actionBusy !== null
                    }
                    onClick={() => void handleHubUpsert()}
                  >
                    {t("settings.skillsDockExtensions.saveHub")}
                  </Button>
                </div>
              </div>
            </SettingCard>
          </div>
        </div>
      </div>
    )
  }
}

function DockSectionNav({
  activeSection,
  onSectionChange,
}: {
  activeSection: DockSection
  onSectionChange: (section: DockSection) => void
}) {
  const { t } = useTranslation()
  return (
    <div className="sticky top-0 z-10 mb-5 rounded-2xl border border-border bg-card/95 p-2 shadow-sm backdrop-blur">
      <div className="grid gap-2 md:grid-cols-4">
        {DOCK_SECTIONS.map((section) => {
          const active = section.id === activeSection
          return (
            <button
              key={section.id}
              type="button"
              className={
                active
                  ? "rounded-xl border border-primary/20 bg-primary px-3 py-3 text-left text-primary-foreground shadow-sm"
                  : "rounded-xl border border-transparent bg-secondary/30 px-3 py-3 text-left text-muted-foreground transition hover:border-border hover:bg-secondary/60 hover:text-foreground"
              }
              onClick={() => onSectionChange(section.id)}
            >
              <div className="text-xs font-semibold">{t(section.labelKey)}</div>
              <div
                className={
                  active
                    ? "mt-1 text-[10px] text-primary-foreground/75"
                    : "mt-1 text-[10px] text-muted-foreground"
                }
              >
                {t(section.descKey)}
              </div>
            </button>
          )
        })}
      </div>
    </div>
  )
}


function UsageMetricCard({
  icon,
  iconClassName,
  title,
  value,
  delta,
}: {
  icon: string
  iconClassName: string
  title: string
  value: number | string
  delta: string
}) {
  return (
    <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <div className={`flex h-10 w-10 items-center justify-center rounded-[10px] text-lg ${iconClassName}`}>
        {icon}
      </div>
      <div className="mt-2 text-xs text-[#888]">{title}</div>
      <div className="mt-1 text-2xl font-bold text-[#1d1d1f] dark:text-foreground">{value}</div>
      <div className="mt-1 line-clamp-1 text-xs font-medium text-[#22c55e]">{delta}</div>
    </section>
  )
}

function UsageTrendLinesCard({
  series,
  granularity,
}: {
  series: UsageTrendSeries
  granularity: "day" | "week"
}) {
  const max = Math.max(1, ...series.apps.flatMap((app) => app.values.map((point) => point.count)))
  const xFor = (index: number) => (series.labels.length <= 1 ? 0 : (index / (series.labels.length - 1)) * 300)
  const yFor = (count: number) => 170 - (count / max) * 145
  return (
    <section className="min-w-0 rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <div className="mb-2 flex items-center justify-between">
        <h3 className="text-sm font-semibold">Skill 调用趋势</h3>
        <span className="text-[11px] text-[#888]">{granularity === "day" ? "按天" : "按周"}</span>
      </div>
      <div className="mb-2 flex flex-wrap gap-x-4 gap-y-1">
        {series.apps.map((app) => (
          <span key={app.app} className={`text-[11px] ${APP_TEXT_CLASS_NAMES[app.app] ?? "text-slate-500"}`}>
            ● {appLabel(app.app)}
          </span>
        ))}
      </div>
      <div className="relative h-[200px] pb-5 pl-8">
        <div className="absolute bottom-5 left-0 top-0 flex flex-col justify-between text-[11px] text-[#aaa]">
          {[max, Math.round(max * 0.75), Math.round(max * 0.5), Math.round(max * 0.25), 0].map(
            (label, index) => (
              <span key={`${label}:${index}`}>{label}</span>
            ),
          )}
        </div>
        <svg className="h-full w-full overflow-visible" viewBox="0 0 300 180" preserveAspectRatio="none">
          {[0, 45, 90, 135, 180].map((y) => (
            <line key={y} x1="0" x2="300" y1={y} y2={y} stroke="#e5e7eb" strokeDasharray="4 4" />
          ))}
          {series.apps.map((app) => (
            <polyline
              key={app.app}
              fill="none"
              stroke={app.color}
              strokeWidth="2.5"
              points={app.values.map((point, index) => `${xFor(index)},${yFor(point.count)}`).join(" ")}
            />
          ))}
        </svg>
        <div className="absolute inset-x-8 bottom-0 flex justify-between text-[11px] text-[#888]">
          {series.labels.map((label) => (
            <span key={label}>{label.slice(5)}</span>
          ))}
        </div>
      </div>
    </section>
  )
}

function UsageAppDistributionCard({ rows }: { rows: Array<{ app: AppKind; count: number }> }) {
  const total = rows.reduce((sum, row) => sum + row.count, 0)
  return (
    <section className="min-w-0 rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <h3 className="mb-3 text-sm font-semibold">应用来源分布</h3>
      <div className="space-y-3">
        {rows.map((row) => {
          const percent = total ? Math.round((row.count / total) * 100) : 0
          return (
            <div key={row.app}>
              <div className="mb-1 flex items-center justify-between text-xs">
                <span className="font-medium">{appLabel(row.app)}</span>
                <span className="text-[#888]">{formatNumber(row.count)} · {percent}%</span>
              </div>
              <div className="h-2 overflow-hidden rounded-full bg-[#f1f5f9]">
                <div
                  className={`h-full rounded-full ${APP_BAR_CLASS_NAMES[row.app] ?? "bg-slate-500"} ${percentWidthClass(percent)}`}
                />
              </div>
            </div>
          )
        })}
        {!rows.length ? <div className="text-xs text-[#888]">暂无调用来源，点击刷新数据后扫描。</div> : null}
      </div>
    </section>
  )
}

function UsageRecentRecordsTable({ rows }: { rows: SkillUsageRecentRecord[] }) {
  return (
    <section className="min-w-0 rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <h3 className="mb-3 text-sm font-semibold">最近调用记录</h3>
      <div className="overflow-hidden rounded-lg border border-[#edf0f2]">
        <div className="grid grid-cols-[1.2fr_1fr_1fr_0.6fr] bg-[#f8fafc] px-3 py-2 text-[11px] font-medium text-[#888]">
          <span>时间</span>
          <span>应用来源</span>
          <span>Skill</span>
          <span className="text-right">次数</span>
        </div>
        {rows.map((row) => (
          <div
            key={`${row.app}:${row.sessionId}:${row.skillName}:${row.activatedAt}`}
            className="grid grid-cols-[1.2fr_1fr_1fr_0.6fr] border-t border-[#edf0f2] px-3 py-2 text-xs"
          >
            <span className="truncate text-[#555]">{formatUsageTime(row.activatedAt)}</span>
            <span className="truncate">{appLabel(row.app)}</span>
            <span className="truncate font-medium">{row.skillName}</span>
            <span className="text-right font-semibold">{row.count}</span>
          </div>
        ))}
        {!rows.length ? <div className="border-t border-[#edf0f2] px-3 py-6 text-center text-xs text-[#888]">暂无最近调用记录。</div> : null}
      </div>
    </section>
  )
}

type UsageTrendSeries = {
  labels: string[]
  apps: Array<{ app: AppKind; color: string; values: Array<{ date: string; count: number }> }>
}

function buildUsageTrendSeries(rows: SkillUsageTrendPoint[]): UsageTrendSeries {
  const labelSet = new Set<string>()
  for (let offset = 6; offset >= 0; offset -= 1) {
    const date = new Date()
    date.setDate(date.getDate() - offset)
    labelSet.add(date.toISOString().slice(0, 10))
  }
  for (const row of rows) labelSet.add(row.date)
  const labels = Array.from(labelSet).sort().slice(-7)
  const apps = (["claude", "codex", "gemini", "opencode", "hope"] as AppKind[])
    .filter((app) => rows.some((row) => row.app === app) || app !== "hope")
    .map((app) => ({
      app,
      color: APP_BAR_COLORS[app] ?? "#64748b",
      values: labels.map((date) => ({
        date,
        count: rows
          .filter((row) => row.app === app && row.date === date)
          .reduce((total, row) => total + row.count, 0),
      })),
    }))
    .filter((app) => app.values.some((point) => point.count > 0) || app.app !== "hope")
  return { labels, apps: apps.length ? apps : [{ app: "hope", color: "#64748b", values: labels.map((date) => ({ date, count: 0 })) }] }
}

function buildUsageAppDistribution(
  rows: SkillUsageAppBreakdown[],
  probes: SkillDockSnapshot["apps"],
): Array<{ app: AppKind; count: number }> {
  const byApp = new Map(rows.map((row) => [row.app, row.count]))
  return (["claude", "codex", "gemini", "opencode", "hope"] as AppKind[])
    .filter((app) => byApp.has(app) || probes.some((probe) => probe.app === app && probe.installed))
    .map((app) => ({ app, count: byApp.get(app) ?? 0 }))
}

function formatNumber(value: number): string {
  return new Intl.NumberFormat().format(value)
}

function formatUsageTime(value: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleString()
}

function daysBetween(value: string, now: Date): number {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return Number.POSITIVE_INFINITY
  return Math.abs(now.getTime() - date.getTime()) / 86_400_000
}

function appLabel(app: AppKind): string {
  if (app === "claude") return "Claude"
  if (app === "codex") return "Codex"
  if (app === "gemini") return "Gemini"
  if (app === "opencode") return "OpenCode"
  return "Hope"
}

function percentWidthClass(percent: number): string {
  if (percent >= 95) return "w-full"
  if (percent >= 85) return "w-11/12"
  if (percent >= 75) return "w-10/12"
  if (percent >= 65) return "w-8/12"
  if (percent >= 55) return "w-7/12"
  if (percent >= 45) return "w-6/12"
  if (percent >= 35) return "w-5/12"
  if (percent >= 25) return "w-4/12"
  if (percent >= 15) return "w-3/12"
  if (percent > 0) return "w-2/12"
  return "w-0"
}

function OverviewStats({
  stats,
  appInstallCounts,
  t,
}: {
  stats: { total: number; usage: number }
  appInstallCounts: Array<{ app: AppKind; installed: number }>
  t: (key: string, options?: Record<string, unknown>) => string
}) {
  return (
    <div className="grid gap-3 md:grid-cols-3 2xl:grid-cols-6">
      <UsageStatCard
        icon="📦"
        title={t("settings.skillsDockExtensions.usageStats.aggregated")}
        value={stats.total}
        delta={t("settings.skillsDockExtensions.usageStats.snapshotDelta", { count: stats.total })}
      />
      {appInstallCounts.map(({ app, installed }) => (
        <UsageStatCard
          key={app}
          icon={appSymbol(app)}
          title={t("settings.skillsDockExtensions.usageStats.installedTo", {
            app: t(`settings.skillsDockExtensions.app.${app}`),
          })}
          value={installed}
          delta={t("settings.skillsDockExtensions.usageStats.snapshotDelta", { count: installed })}
        />
      ))}
      <UsageStatCard
        icon="📈"
        title={t("settings.skillsDockExtensions.metrics.usage")}
        value={stats.usage}
        delta={t("settings.skillsDockExtensions.usageStats.snapshotDelta", { count: stats.usage })}
      />
    </div>
  )
}

function UsageStatCard({
  icon,
  title,
  value,
  delta,
}: {
  icon: string
  title: string
  value: number | string
  delta: string
}) {
  return (
    <section className="rounded-[10px] border border-[#e5e7eb] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:border-border dark:bg-card">
      <div className="flex items-center gap-2 text-xs text-[#666]">
        <span>{icon}</span>
        <span className="truncate">{title}</span>
      </div>
      <div className="mt-3 text-[28px] font-bold leading-none text-[#1d1d1f] dark:text-foreground">
        {value}
      </div>
      <div className="mt-2 text-xs font-medium text-[#22c55e]">{delta}</div>
    </section>
  )
}

function ScanStatusCard({
  t,
  generatedAtLabel,
  stats,
  activeUsageRows,
  verificationLabel,
  failedInstallCount,
  busy,
  onRefresh,
}: {
  t: (key: string, options?: Record<string, unknown>) => string
  generatedAtLabel: string
  stats: { dirs: number; total: number; user: number }
  activeUsageRows: number
  verificationLabel: string
  failedInstallCount: number
  busy: boolean
  onRefresh: () => void
}) {
  return (
    <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <h3 className="text-sm font-semibold">
        {t("settings.skillsDockExtensions.usageScanStatusTitle")}
      </h3>
      <div className="mt-4 flex items-start gap-3">
        <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-[#dcfce7] text-[#22c55e]">
          <CheckCircle2 className="h-5 w-5" />
        </span>
        <div>
          <div className="text-sm font-semibold">
            {t("settings.skillsDockExtensions.usageScanCompleted")}
          </div>
          <p className="mt-1 text-xs text-[#888]">
            {t("settings.skillsDockExtensions.usageScanCompletedDesc")}
          </p>
        </div>
      </div>
      <dl className="mt-4 space-y-2 text-xs">
        <UsageKv
          label={t("settings.skillsDockExtensions.usageScanTime")}
          value={generatedAtLabel}
        />
        <UsageKv label={t("settings.skillsDockExtensions.usageScanDirs")} value={stats.dirs} />
        <UsageKv label={t("settings.skillsDockExtensions.usageScanFound")} value={stats.total} />
        <UsageKv label={t("settings.skillsDockExtensions.usageScanNew")} value={stats.user} />
        <UsageKv
          label={t("settings.skillsDockExtensions.usageScanUpdated")}
          value={activeUsageRows}
        />
        <UsageKv
          label={t("settings.skillsDockExtensions.usageScanVerification")}
          value={verificationLabel}
          valueClassName={failedInstallCount ? "text-[#f59e0b]" : "text-[#22c55e]"}
        />
      </dl>
      <Button
        className="mt-4 h-9 w-full gap-2 bg-[#2563eb] text-xs text-white"
        disabled={busy}
        onClick={onRefresh}
      >
        <RotateCw className="h-3.5 w-3.5" />
        {t("settings.skillsManager.reload")}
      </Button>
    </section>
  )
}

function AppInstallBarCard({
  counts,
  t,
}: {
  counts: Array<{ app: AppKind; installed: number }>
  t: (key: string) => string
}) {
  return (
    <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <h3 className="text-sm font-semibold">
        {t("settings.skillsDockExtensions.usageAppInstalledTitle")}
      </h3>
      <div className="mt-4 grid h-48 grid-cols-[2rem_minmax(0,1fr)] gap-3">
        <div className="flex flex-col justify-between text-right text-[10px] text-[#aaa]">
          {[60, 45, 30, 15, 0].map((tick) => (
            <span key={tick}>{tick}</span>
          ))}
        </div>
        <div className="flex items-end justify-around gap-5 border-l border-b border-[#e5e7eb] px-2 pb-2">
          {counts.map(({ app, installed }) => (
            <div key={app} className="flex min-w-0 flex-1 flex-col items-center justify-end gap-2">
              <div className="text-xs font-semibold">{installed}</div>
              <div
                className="w-8 rounded-t"
                style={{
                  height: `${Math.max(4, Math.min(100, (installed / 60) * 100))}%`,
                  backgroundColor: APP_BAR_COLORS[app],
                }}
              />
              <div className="truncate text-[11px] text-[#666]">
                {t(`settings.skillsDockExtensions.app.${app}`)}
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}

function TrendCard({
  points,
  max,
  t,
}: {
  points: Array<{ label: string; count: number }>
  max: number
  t: (key: string) => string
}) {
  return (
    <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <div className="mb-4 flex items-center justify-between">
        <h3 className="text-sm font-semibold">
          {t("settings.skillsDockExtensions.usageSevenDayTrendTitle")}
        </h3>
        <span className="rounded border border-[#ddd] px-1.5 py-0.5 text-xs text-[#666]">
          {t("settings.skillsDockExtensions.usageSevenDays")} ▼
        </span>
      </div>
      <div className="relative h-48 pb-6">
        <svg
          className="h-full w-full overflow-visible"
          viewBox="0 0 300 180"
          preserveAspectRatio="none"
        >
          <polyline
            fill="none"
            stroke="#3b82f6"
            strokeWidth="3"
            points={points
              .map((point, index) => `${(index / 6) * 300},${180 - (point.count / max) * 160}`)
              .join(" ")}
          />
          {points.map((point, index) => (
            <circle
              key={point.label}
              cx={(index / 6) * 300}
              cy={180 - (point.count / max) * 160}
              r="4"
              fill="#3b82f6"
            />
          ))}
        </svg>
        <div className="absolute inset-x-0 bottom-0 flex justify-between text-[10px] text-[#888]">
          {points.map((point) => (
            <span key={point.label}>{point.label}</span>
          ))}
        </div>
      </div>
    </section>
  )
}

function TopSkillsCard({
  rows,
  skills,
  t,
  onViewAll,
}: {
  rows: SkillUsageSnapshot[]
  skills: SkillSummary[]
  t: (key: string) => string
  onViewAll: () => void
}) {
  return (
    <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <div className="mb-3 flex justify-between text-xs">
        <h3 className="text-sm font-semibold">
          {t("settings.skillsDockExtensions.usageTopSkillsTitle")}
        </h3>
        <span className="text-[#888]">{t("settings.skillsManager.usageCount")}</span>
      </div>
      <div className="space-y-3">
        {rows.map((row, index) => {
          const skill = skills.find((item) => item.name === row.skillName)
          return (
            <div key={row.skillName} className="flex items-center gap-3 text-xs">
              <span className={`w-5 shrink-0 text-center text-[13px] ${rankTextClassName(index)}`}>
                {index + 1}
              </span>
              <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-[#f5f5f7] text-sm">
                {skill?.display?.emoji || "✦"}
              </span>
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px] font-medium">{row.skillName}</div>
                <div className="truncate text-[11px] text-[#888]">
                  {skill?.description || t("settings.skillsDockExtensions.usageSkillFallbackDesc")}
                </div>
              </div>
              <div className="font-semibold">{row.usageCount}</div>
            </div>
          )
        })}
      </div>
      <button type="button" className="mt-4 text-xs font-medium text-[#2563eb]" onClick={onViewAll}>
        {t("settings.skillsDockExtensions.usageViewAll")}
      </button>
    </section>
  )
}

function TimelineCard({
  rows,
  t,
  onRefresh,
}: {
  rows: ReturnType<typeof buildUsageTimelineRows>
  t: (key: string) => string
  onRefresh: () => void
}) {
  return (
    <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <h3 className="text-sm font-semibold">
        {t("settings.skillsDockExtensions.usageRecentChangesTitle")}
      </h3>
      <div className="mt-4 space-y-4">
        {rows.map((item, index) => (
          <div key={`${item.labelKey}:${item.title}:${item.time}:${index}`} className="flex gap-3 text-xs">
            <span className={`mt-0.5 rounded px-1.5 py-px text-xs ${item.className}`}>
              {t(item.labelKey)}
            </span>
            <div className="min-w-0 flex-1">
              <div className="flex justify-between gap-2">
                <span className="truncate text-[13px] font-medium">{item.title}</span>
                <span className="shrink-0 text-[11px] text-[#888]">{item.time}</span>
              </div>
              <p className="mt-1 truncate text-[11px] text-[#888]">{t(item.descKey)}</p>
            </div>
          </div>
        ))}
      </div>
      <button type="button" className="mt-4 text-xs font-medium text-[#2563eb]" onClick={onRefresh}>
        {t("settings.skillsDockExtensions.usageViewAllChanges")}
      </button>
    </section>
  )
}

function QuickActionsCard({
  t,
  onQuickImport,
  onExport,
  onAddDir,
  onLogs,
}: {
  t: (key: string) => string
  onQuickImport: () => void
  onExport: () => void
  onAddDir: () => void
  onLogs: () => void
}) {
  return (
    <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <h3 className="text-sm font-semibold">
        {t("settings.skillsDockExtensions.usageQuickActionsTitle")}
      </h3>
      <div className="mt-4 space-y-3">
        <UsageQuickButton
          icon={<Upload className="h-4 w-4" />}
          label={t("settings.skillsDockExtensions.importZip")}
          onClick={onQuickImport}
        />
        <UsageQuickButton
          icon={<Download className="h-4 w-4" />}
          label={t("settings.skillsDockExtensions.exportZip")}
          onClick={onExport}
        />
        <UsageQuickButton
          icon={<FolderOpen className="h-4 w-4" />}
          label={t("settings.addSkillsDir")}
          onClick={onAddDir}
        />
        <UsageQuickButton
          icon={<Clock3 className="h-4 w-4" />}
          label={t("settings.skillsDockExtensions.usageOpenLogSource")}
          onClick={onLogs}
        />
      </div>
    </section>
  )
}

function OverviewFooter({
  t,
  stats,
  installed,
  notInstalled,
  failed,
}: {
  t: (key: string, options?: Record<string, unknown>) => string
  stats: { total: number }
  installed: number
  notInstalled: number
  failed: number
}) {
  return (
    <footer className="flex flex-col gap-2 border-t border-[#e5e7eb] pt-3 text-xs text-[#888] lg:flex-row lg:items-center lg:justify-between">
      <div className="flex flex-wrap gap-x-4 gap-y-1">
        <span>
          {t("settings.skillsDockExtensions.usageFooterAggregated", { count: stats.total })}
        </span>
        <span>{t("settings.skillsDockExtensions.usageFooterInstalled", { count: installed })}</span>
        <span>
          {t("settings.skillsDockExtensions.usageFooterNotInstalled", { count: notInstalled })}
        </span>
        <span>{t("settings.skillsDockExtensions.usageFooterFailed", { count: failed })}</span>
      </div>
      <IconTip label={t("settings.skillsDockExtensions.usageLogSourceTip")}>
        <span className="cursor-help">{t("settings.skillsDockExtensions.usageLogSource")}</span>
      </IconTip>
    </footer>
  )
}

function UsageKv({
  label,
  value,
  valueClassName,
}: {
  label: string
  value: number | string
  valueClassName?: string
}) {
  return (
    <div className="flex items-center justify-between gap-4 border-b border-[#e5e7eb]/70 pb-2 last:border-0">
      <dt className="text-[#666]">{label}</dt>
      <dd
        className={`min-w-0 truncate text-right font-medium ${valueClassName ?? "text-[#1d1d1f]"}`}
      >
        {value}
      </dd>
    </div>
  )
}

function UsageQuickButton({
  icon,
  label,
  onClick,
}: {
  icon: ReactNode
  label: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      className="flex w-full items-center gap-3 rounded-lg border border-[#e5e7eb] bg-[#f5f5f7] px-3 py-3 text-left text-xs font-medium text-[#1d1d1f]"
      onClick={onClick}
    >
      <span className="text-[#2563eb]">{icon}</span>
      <span>{label}</span>
    </button>
  )
}

interface MarketCategoryItem {
  icon: string
  label: string
  count: number
  className: string
}

interface MarketDisplayItem {
  id: string
  kind: "remote" | "registry"
  name: string
  version: string
  description: string
  category: string
  author: string
  sourceName: string
  rating: string
  downloads: string
  icon: string
  iconClassName: string
  installed: boolean
  updateAvailable: boolean
  busyKey: string
  rankScore: number
  updatedLabel: string
  updateAction: string
  remoteEntry?: SkillRemoteMarketEntry
  registryEntry?: SkillRegistryEntry
}

function toRemoteMarketDisplayItem(
  entry: SkillRemoteMarketEntry,
  index: number,
): MarketDisplayItem {
  return {
    id: `remote:${entry.id}`,
    kind: "remote",
    name: entry.name,
    version: entry.marketVersion ? `v${entry.marketVersion.replace(/^v/, "")}` : "latest",
    description: entry.description,
    category: entry.category,
    author: entry.author,
    sourceName: entry.sourceName,
    rating: entry.rating.toFixed(1),
    downloads: formatMarketDownloadCount(entry.downloadCount),
    icon: marketIconFor(entry.category, index),
    iconClassName: marketIconClassFor(entry.category),
    installed: entry.installed,
    updateAvailable: entry.updateAvailable,
    busyKey: `market:${entry.id}`,
    rankScore:
      entry.downloadCount + Math.round(entry.rating * 100) + Number(entry.featured) * 10000,
    updatedLabel: entry.updatedAt ? new Date(entry.updatedAt).toLocaleString() : "最近更新",
    updateAction: entry.updateAvailable
      ? `更新到 ${entry.marketVersion || "latest"}`
      : entry.installed
        ? "已安装"
        : "新上架",
    remoteEntry: entry,
  }
}

function toRegistryMarketDisplayItem(entry: SkillRegistryEntry, index: number): MarketDisplayItem {
  return {
    id: `registry:${entry.id}`,
    kind: "registry",
    name: entry.name,
    version: entry.version || "registry",
    description: entry.description || entry.sourcePath || entry.skillPath,
    category: entry.category,
    author: entry.sourceId,
    sourceName: entry.sourceId,
    rating: "0.0",
    downloads: "0",
    icon: marketIconFor(entry.category, index),
    iconClassName: marketIconClassFor(entry.category),
    installed: entry.installed,
    updateAvailable: entry.updateAvailable,
    busyKey: `registry:${entry.id}`,
    rankScore: Number(entry.updateAvailable) * 1000 + Number(entry.installed) * 100,
    updatedLabel: entry.updatedAt ? new Date(entry.updatedAt).toLocaleString() : "本地 Registry",
    updateAction: entry.updateAvailable ? "更新可用" : entry.installed ? "已安装" : "新上架",
    registryEntry: entry,
  }
}

function marketIconFor(category: string, index: number) {
  const icons: Record<string, string> = {
    开发: "💻",
    效率: "⚡",
    文档: "📄",
    测试: "🧪",
    设计: "🎨",
    自动化: "🤖",
    数据: "🗄️",
    团队协作: "👥",
  }
  return icons[category] || ["💻", "🧪", "🚀", "👁️", "📝", "🎨"][index % 6]
}

function marketIconClassFor(category: string) {
  const classes: Record<string, string> = {
    开发: "bg-[#dbeafe] text-[#1e40af]",
    效率: "bg-[#fef3c7] text-[#92400e]",
    文档: "bg-[#dcfce7] text-[#166534]",
    测试: "bg-[#fee2e2] text-[#991b1b]",
    设计: "bg-[#f3e8ff] text-[#6b21a8]",
    自动化: "bg-[#e0f2fe] text-[#075985]",
    数据: "bg-[#ecfdf5] text-[#047857]",
    团队协作: "bg-[#f1f5f9] text-[#334155]",
  }
  return classes[category] || "bg-[#eef2ff] text-[#3730a3]"
}

function formatMarketDownloadCount(value: number) {
  if (value >= 1000) return `${(value / 1000).toFixed(1)}k`
  return String(value)
}

function MarketFilterSelect({
  value,
  onChange,
  label,
  options,
  optionValues,
}: {
  value: string
  onChange: (value: string) => void
  label: string
  options: string[]
  optionValues?: string[]
}) {
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger className="h-9 w-32 rounded-md border-[#e5e7eb] bg-white px-3 text-[13px] text-[#374151] shadow-none dark:bg-card">
        <SelectValue placeholder={label} />
      </SelectTrigger>
      <SelectContent>
        {options.map((option, index) => (
          <SelectItem key={optionValues?.[index] ?? option} value={optionValues?.[index] ?? option}>
            {option}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

function MarketCategoryCard({ item }: { item: MarketCategoryItem }) {
  return (
    <div className="flex min-w-max items-center gap-2 rounded-lg bg-white px-3.5 py-2.5 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <span
        className={`flex h-7 w-7 items-center justify-center rounded-lg text-sm ${item.className}`}
      >
        {item.icon}
      </span>
      <div>
        <div className="text-[13px] font-medium leading-4 text-[#1d1d1f] dark:text-foreground">
          {item.label}
        </div>
        <div className="mt-0.5 text-xs leading-4 text-[#888]">{item.count} 个</div>
      </div>
    </div>
  )
}

function MarketSkillCard({
  item,
  busy,
  onAction,
}: {
  item: MarketDisplayItem
  index: number
  busy: boolean
  onAction: () => void
}) {
  const { t } = useTranslation()
  const disabled = item.installed && !item.updateAvailable
  return (
    <article className="flex min-h-[172px] flex-col rounded-[10px] bg-white p-3.5 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 gap-3">
          <span
            className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-lg text-lg ${item.iconClassName}`}
          >
            {item.icon}
          </span>
          <div className="min-w-0">
            <h4 className="truncate text-sm font-semibold leading-5 text-[#1d1d1f] dark:text-foreground">
              {item.name}
            </h4>
            <div className="mt-0.5 flex items-center gap-2 text-[13px] leading-5">
              <span className="text-[#f59e0b]">{item.rating} ★</span>
              <span className="text-[#888]">{item.downloads}</span>
            </div>
          </div>
        </div>
        <span className="text-[#aaa]">{t("settings.skillsDockExtensions.marketMore")}</span>
      </div>
      <p className="mt-1 line-clamp-2 text-xs leading-5 text-[#888]">{item.description}</p>
      <div className="mt-1.5 flex items-center gap-1.5">
        <span className="rounded bg-[#dcfce7] px-1.5 py-px text-[11px] leading-4 text-[#166534]">
          {item.category}
        </span>
        <span className="truncate text-xs leading-4 text-[#888]">by {item.author}</span>
      </div>
      <div className="mt-auto flex items-center justify-between gap-3 pt-2.5">
        <div className="flex gap-1.5">
          <MarketAppDots />
        </div>
        <button
          type="button"
          className={
            item.installed && !item.updateAvailable
              ? "rounded-md bg-[#f3f4f6] px-3.5 py-1 text-xs font-medium text-[#9ca3af]"
              : "rounded-md bg-[#2563eb] px-3.5 py-1 text-xs font-medium text-white hover:bg-[#1d4ed8] disabled:opacity-60"
          }
          disabled={busy || disabled}
          onClick={onAction}
        >
          {item.updateAvailable
            ? t("settings.skillsDockExtensions.update")
            : item.installed
              ? t("settings.skillsDockExtensions.installed")
              : t("settings.skillsDockExtensions.install")}
        </button>
      </div>
    </article>
  )
}

function MarketUpdateCard({ item, index }: { item: MarketDisplayItem; index: number }) {
  return (
    <div className="min-w-[200px] rounded-lg bg-white p-3 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <div className="flex items-center gap-2 text-[13px] font-medium leading-5 text-[#1d1d1f] dark:text-foreground">
        <span
          className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-base ${item.iconClassName}`}
        >
          {["🧪", "💻", "🚀", "📝"][index % 4] || item.icon}
        </span>
        <div className="min-w-0">
          <div className="truncate">{item.name}</div>
          <div className="text-xs font-normal text-[#2563eb]">{item.updateAction}</div>
        </div>
      </div>
      <p className="mt-1 line-clamp-2 text-xs leading-5 text-[#888]">{item.description}</p>
      <div className="mt-1 text-[11px] leading-4 text-[#aaa]">{item.updatedLabel}</div>
    </div>
  )
}

function MarketStatsCard({
  title,
  items,
}: {
  title: string
  items: Array<{ icon: string; label: string; value: number; unit: string }>
}) {
  return (
    <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <h3 className="mb-3 text-sm font-semibold text-[#1d1d1f] dark:text-foreground">{title}</h3>
      <div className="grid grid-cols-3 gap-2">
        {items.map((item) => (
          <div key={item.label} className="min-w-0 text-center">
            <div className="mx-auto mb-1 flex h-8 w-8 items-center justify-center rounded-lg bg-[#eff6ff] text-base">
              {item.icon}
            </div>
            <div className="text-[11px] leading-4 text-[#888]">{item.label}</div>
            <div className="mt-0.5 text-lg font-bold leading-6 text-[#1d1d1f] dark:text-foreground">
              {item.value.toLocaleString()}
            </div>
            <div className="text-xs leading-4 text-[#888]">{item.unit}</div>
          </div>
        ))}
      </div>
    </section>
  )
}

function MarketRankRow({ item, index }: { item: MarketDisplayItem; index: number }) {
  return (
    <div className="flex items-center gap-2">
      <span className="w-4 shrink-0 text-xs font-semibold text-[#1d1d1f] dark:text-foreground">
        {index + 1}
      </span>
      <span
        className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-sm ${item.iconClassName}`}
      >
        {item.icon}
      </span>
      <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-[#1d1d1f] dark:text-foreground">
        {item.name}
      </span>
      <span className="shrink-0 text-xs text-[#888]">⬇️ {item.downloads}</span>
    </div>
  )
}

function MarketPreviewCard({
  item,
  busy,
  onAction,
}: {
  item: MarketDisplayItem
  busy: boolean
  onAction: () => void
}) {
  const { t } = useTranslation()
  const disabled = item.installed && !item.updateAvailable
  return (
    <section className="rounded-[10px] bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <h3 className="mb-3 text-sm font-semibold text-[#1d1d1f] dark:text-foreground">
        {t("settings.skillsDockExtensions.skillPreviewTitle")}
      </h3>
      <div className="flex gap-2.5">
        <span className="flex h-10 w-10 items-center justify-center rounded-lg bg-[#dcfce7] text-lg text-[#166534]">
          &lt;/&gt;
        </span>
        <div>
          <div className="text-sm font-semibold leading-5 text-[#1d1d1f] dark:text-foreground">
            {item.name}
          </div>
          <span className="rounded bg-[#f3f4f6] px-1.5 py-px text-[11px] text-[#6b7280]">
            {item.version}
          </span>
        </div>
      </div>
      <p className="mt-1.5 line-clamp-4 text-xs leading-5 text-[#888]">
        {item.description} {t("settings.skillsDockExtensions.skillPreviewSuffix")}
      </p>
      <div className="mt-2.5 flex gap-2">
        <MarketAppDots size="lg" />
      </div>
      <div className="mt-2.5 flex flex-wrap gap-x-3 gap-y-1 text-[13px] leading-5">
        <span className="text-[#f59e0b]">{item.rating} ★</span>
        <span className="text-[#888]">
          {t("settings.skillsDockExtensions.downloads", { count: item.downloads })}
        </span>
        <span className="text-[#888]">
          {t("settings.skillsDockExtensions.lastUpdatedToday")}
        </span>
      </div>
      <div className="mt-3 flex gap-2.5">
        <button
          type="button"
          className="flex-1 rounded-md bg-[#2563eb] px-3 py-2 text-center text-xs font-medium text-white hover:bg-[#1d4ed8] disabled:bg-[#f3f4f6] disabled:text-[#9ca3af]"
          disabled={busy || disabled}
          onClick={onAction}
        >
          {item.updateAvailable
            ? t("settings.skillsDockExtensions.updateToApp")
            : item.installed
              ? t("settings.skillsDockExtensions.installed")
              : t("settings.skillsDockExtensions.installToApp")}
        </button>
        <button
          type="button"
          className="flex-1 rounded-md border border-[#e5e7eb] bg-white px-3 py-2 text-center text-xs font-medium text-[#374151]"
        >
          {t("settings.skillsDockExtensions.viewDetails")}
        </button>
      </div>
    </section>
  )
}

function MarketAppDots({ size = "sm" }: { size?: "sm" | "lg" }) {
  const cls = size === "lg" ? "h-5 w-5 text-[10px]" : "h-4 w-4 text-[9px]"
  return (
    <>
      {[
        ["C", "bg-[#ffedd5] text-[#f97316]"],
        ["X", "bg-[#f3f4f6] text-[#6b7280]"],
        ["G", "bg-[#dcfce7] text-[#22c55e]"],
        ["O", "bg-[#dbeafe] text-[#2563eb]"],
      ].map(([label, color]) => (
        <span key={label} className={`flex items-center justify-center rounded ${cls} ${color}`}>
          {label}
        </span>
      ))}
    </>
  )
}

function ImportOption({ title, desc }: { title: string; desc: string }) {
  return (
    <div className="flex gap-3">
      <span className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-[3px] bg-[#2563eb] text-[10px] text-white">
        ✓
      </span>
      <div>
        <div className="text-[13px] font-medium text-[#1d1d1f]">{title}</div>
        <div className="mt-0.5 text-xs text-[#888]">{desc}</div>
      </div>
    </div>
  )
}

function ImportStatus({ status }: { status: "ok" | "warning" }) {
  if (status === "warning") {
    return (
      <span className="inline-flex items-center gap-1 text-xs font-medium text-[#d97706]">
        ▲ 警告
      </span>
    )
  }
  return (
    <span className="inline-flex items-center gap-1 text-xs font-medium text-[#16a34a]">
      ● 通过
    </span>
  )
}

function InstallMark({ installed }: { installed: boolean }) {
  return installed ? (
    <span className="inline-flex h-5 w-5 items-center justify-center rounded-full bg-[#dcfce7] text-xs text-[#22c55e]">
      ✓
    </span>
  ) : (
    <span className="inline-flex h-5 w-5 items-center justify-center rounded-full bg-[#f3f4f6] text-xs text-[#d1d5db]">
      −
    </span>
  )
}

function ExportSummaryRow({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="flex items-center justify-between gap-3 py-1.5 text-xs">
      <span className="text-[#888]">📄 {label}</span>
      <span className="text-[13px] font-semibold text-[#1d1d1f]">{value}</span>
    </div>
  )
}

function RecentIoRow({
  row,
}: {
  row: {
    type: "import" | "export"
    file: string
    content: string
    time: string
    status: "success" | "warning"
  }
}) {
  return (
    <div className="flex items-center gap-3 border-b border-[#f0f0f0] py-2 last:border-0">
      <span className={row.type === "import" ? "text-[#22c55e]" : "text-[#3b82f6]"}>
        {row.type === "import" ? "⬇" : "⬆"}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 text-[13px]">
          <span className="text-[#374151]">{row.type === "import" ? "导入" : "导出"}</span>
          <span className="truncate font-medium">{row.file}</span>
        </div>
        <div className="mt-0.5 truncate text-xs text-[#888]">
          {row.content} · {row.time}
        </div>
      </div>
      <span
        className={
          row.status === "success"
            ? "rounded bg-[#dcfce7] px-2 py-0.5 text-[11px] text-[#166534]"
            : "rounded bg-[#fef3c7] px-2 py-0.5 text-[11px] text-[#92400e]"
        }
      >
        {row.status === "success" ? "成功" : "警告"}
      </span>
    </div>
  )
}

function formatBytes(bytes: number): string {
  if (!bytes) return "-"
  const mb = bytes / 1024 / 1024
  if (mb >= 1) return `${mb.toFixed(1)} MB`
  const kb = bytes / 1024
  return `${Math.max(1, Math.round(kb))} KB`
}

function SettingTopButton({ children, onClick }: { children: ReactNode; onClick: () => void }) {
  return (
    <button
      type="button"
      className="rounded-md border border-[#e5e7eb] bg-white px-3.5 py-1.5 text-[13px] text-[#374151] shadow-[0_1px_3px_rgba(0,0,0,0.05)] hover:bg-[#f9fafb] dark:bg-card dark:text-foreground"
      onClick={onClick}
    >
      {children}
    </button>
  )
}

function SettingCard({
  title,
  desc,
  action,
  children,
}: {
  title: string
  desc: string
  action?: ReactNode
  children: ReactNode
}) {
  return (
    <section className="rounded-[10px] bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.05)] dark:bg-card">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold">{title}</h3>
          <p className="mt-1 text-xs text-[#888]">{desc}</p>
        </div>
        {action && <div className="shrink-0">{action}</div>}
      </div>
      {children}
    </section>
  )
}

function SettingButton({
  children,
  disabled,
  onClick,
}: {
  children: ReactNode
  disabled?: boolean
  onClick?: () => void
}) {
  return (
    <button
      type="button"
      className="shrink-0 rounded-md border border-[#e5e7eb] bg-white px-2.5 py-1 text-xs text-[#555] disabled:opacity-50"
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </button>
  )
}

function SettingPathInput({ value, title }: { value: string; title?: string }) {
  return (
    <div
      className="min-w-0 flex-1 truncate rounded-md border border-[#e5e7eb] bg-[#fafafa] px-2.5 py-1.5 text-[13px] text-[#666]"
      title={title ?? value}
    >
      {value}
    </div>
  )
}

function SettingSwitch({ checked }: { checked: boolean }) {
  return (
    <span
      className={`relative inline-flex h-5 w-9 rounded-full ${checked ? "bg-[#2563eb]" : "bg-[#d1d5db]"}`}
    >
      <span
        className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow ${checked ? "left-[18px]" : "left-0.5"}`}
      />
    </span>
  )
}

function SettingToggleRow({
  title,
  desc,
  checked,
}: {
  title: string
  desc: string
  checked: boolean
}) {
  return (
    <div className="flex items-center justify-between gap-4 border-b border-[#f0f0f0] py-3 last:border-0">
      <div className="min-w-0">
        <div className="text-[13px] font-medium">{title}</div>
        <div className="mt-0.5 text-xs text-[#888]">{desc}</div>
      </div>
      <SettingSwitch checked={checked} />
    </div>
  )
}

function SettingSelectRow({ title, desc, value }: { title: string; desc: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-4 border-b border-[#f0f0f0] py-3 last:border-0">
      <div className="min-w-0">
        <div className="text-[13px] font-medium">{title}</div>
        <div className="mt-0.5 text-xs text-[#888]">{desc}</div>
      </div>
      <span className="shrink-0 rounded-md border border-[#e5e7eb] bg-white px-2.5 py-1 text-xs text-[#555]">
        {value} ▼
      </span>
    </div>
  )
}

function SettingAppDirRow({
  row,
  t,
}: {
  row: { app: Exclude<AppKind, "hope">; skillPath: string; scanned: boolean }
  t: (key: string) => string
}) {
  return (
    <div className="flex min-w-0 items-center gap-2 py-1">
      <div className="flex w-24 shrink-0 items-center gap-2 text-[13px] font-medium">
        <span style={{ color: APP_BAR_COLORS[row.app] }}>{appSymbol(row.app)}</span>
        <span>{t(`settings.skillsDockExtensions.app.${row.app}`)}</span>
      </div>
      <SettingPathInput value={shortPath(row.skillPath)} title={row.skillPath} />
      <span className="rounded-md border border-dashed border-[#d1d5db] px-2.5 py-1 text-xs text-[#9ca3af]">
        {t("settings.skillsDockExtensions.browse")}
      </span>
      <span className="shrink-0 rounded bg-[#dcfce7] px-2 py-0.5 text-[11px] text-[#166534]">
        {row.scanned
          ? t("settings.skillsDockExtensions.scanned")
          : t("settings.skillsDockExtensions.notScanned")}
      </span>
    </div>
  )
}

function SettingLogRow({
  row,
  t,
}: {
  row: { app: Exclude<AppKind, "hope">; logPath: string }
  t: (key: string) => string
}) {
  return (
    <div className="grid grid-cols-[5rem_minmax(0,1fr)_3.5rem] items-center gap-2">
      <div className="flex items-center gap-1.5 text-[13px] font-medium">
        <span style={{ color: APP_BAR_COLORS[row.app] }}>{appSymbol(row.app)}</span>
        <span>{t(`settings.skillsDockExtensions.app.${row.app}`)}</span>
      </div>
      <div className="flex min-w-0 gap-2">
        <SettingPathInput value={shortPath(row.logPath)} title={row.logPath} />
        <span className="rounded-md border border-dashed border-[#d1d5db] px-2.5 py-1 text-xs text-[#9ca3af]">
          {t("settings.skillsDockExtensions.browse")}
        </span>
      </div>
      <div className="flex justify-end">
        <SettingSwitch checked />
      </div>
    </div>
  )
}

function HubTokenCard({
  hub,
  tokenDraft,
  tokenStatus,
  busy,
  t,
  onDraft,
  onSave,
  onClear,
  onToggle,
}: {
  hub: SkillMarketHubConfigFile["hubs"][number]
  tokenDraft: string
  tokenStatus: string
  busy: boolean
  t: (key: string) => string
  onDraft: (value: string) => void
  onSave: () => void
  onClear: () => void
  onToggle: () => void
}) {
  return (
    <div className="rounded-lg border border-[#e5e7eb] bg-[#fafafa] p-3 text-xs">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="truncate font-medium">{hub.name}</div>
          <div className="truncate text-[11px] text-[#888]">{hub.baseUrl}</div>
        </div>
        <span className="shrink-0 rounded bg-[#f5f5f7] px-2 py-0.5 text-[11px] text-[#666]">
          {hub.readOnly
            ? t("settings.skillsDockExtensions.readOnly")
            : hub.enabled
              ? t("settings.skillsDockExtensions.enabled")
              : t("settings.skillsDockExtensions.disabled")}
        </span>
      </div>
      {!hub.readOnly && (
        <div className="mt-2 grid gap-2 md:grid-cols-[minmax(0,1fr)_auto_auto_auto]">
          <Input
            type="password"
            value={tokenDraft}
            onChange={(event) => onDraft(event.target.value)}
            placeholder={tokenStatus || t("settings.skillsDockExtensions.tokenPlaceholder")}
            className="h-8 text-xs"
          />
          <SettingButton disabled={!tokenDraft.trim() || busy} onClick={onSave}>
            {t("settings.skillsDockExtensions.saveToken")}
          </SettingButton>
          <SettingButton disabled={busy} onClick={onClear}>
            {t("settings.skillsDockExtensions.clearToken")}
          </SettingButton>
          <SettingButton disabled={busy} onClick={onToggle}>
            {hub.enabled
              ? t("settings.skillsDockExtensions.disable")
              : t("settings.skillsDockExtensions.enable")}
          </SettingButton>
        </div>
      )}
    </div>
  )
}

function buildUsageTrendPoints(
  rows: SkillUsageSnapshot[],
): Array<{ label: string; count: number }> {
  const today = new Date()
  return Array.from({ length: 7 }, (_, index) => {
    const date = new Date(today)
    date.setDate(today.getDate() - (6 - index))
    const key = date.toISOString().slice(0, 10)
    const count = rows
      .filter((row) => row.lastUsedAt?.slice(0, 10) === key)
      .reduce((total, row) => total + row.usageCount, 0)
    return { label: `${date.getMonth() + 1}/${date.getDate()}`, count }
  })
}

function buildUsageTimelineRows(rows: SkillUsageSnapshot[], fallbackTime: string) {
  const used = rows.filter((row) => row.usageCount > 0)
  const conflicts = rows.filter((row) =>
    row.apps.some((state) => state.state === "conflict" || state.state === "attention"),
  )
  return [
    ...(used[0]
      ? [
          {
            labelKey: "settings.skillsDockExtensions.usageTimeline.used",
            title: used[0].skillName,
            time: used[0].lastUsedAt ? new Date(used[0].lastUsedAt).toLocaleString() : fallbackTime,
            descKey: "settings.skillsDockExtensions.usageTimeline.usedDesc",
            className: "bg-[#22c55e]/10 text-[#16a34a]",
          },
        ]
      : []),
    ...(rows[0]
      ? [
          {
            labelKey: "settings.skillsDockExtensions.usageTimeline.scanned",
            title: rows[0].skillName,
            time: fallbackTime,
            descKey: "settings.skillsDockExtensions.usageTimeline.scannedDesc",
            className: "bg-[#3b82f6]/10 text-[#2563eb]",
          },
        ]
      : []),
    ...(conflicts[0]
      ? [
          {
            labelKey: "settings.skillsDockExtensions.usageTimeline.warning",
            title: conflicts[0].skillName,
            time: fallbackTime,
            descKey: "settings.skillsDockExtensions.usageTimeline.warningDesc",
            className: "bg-[#f59e0b]/10 text-[#d97706]",
          },
        ]
      : [
          {
            labelKey: "settings.skillsDockExtensions.usageTimeline.passed",
            title: rows[0]?.skillName ?? "-",
            time: fallbackTime,
            descKey: "settings.skillsDockExtensions.usageTimeline.passedDesc",
            className: "bg-[#22c55e]/10 text-[#16a34a]",
          },
        ]),
  ]
}

function defaultSkillPath(app: Exclude<AppKind, "hope">): string {
  if (app === "claude") return "~/Library/Application Support/Claude/skills"
  if (app === "codex") return "~/.codex/skills"
  if (app === "gemini") return "~/Library/Application Support/Google/Gemini/skills"
  return "~/.opencode/skills"
}

function defaultLogPath(app: Exclude<AppKind, "hope">): string {
  if (app === "claude") return "~/Library/Application Support/Claude/logs"
  if (app === "codex") return "~/.codex/logs"
  if (app === "gemini") return "~/Library/Application Support/Google/Gemini/logs"
  return "~/.opencode/logs"
}

function appSymbol(app: AppKind): string {
  if (app === "claude") return "☀️"
  if (app === "gemini") return "✦"
  return "⬡"
}

function shortPath(path: string): string {
  return path.replace(/^C:\\Users\\[^\\]+/i, "~").replace(/^\/Users\/[^/]+/i, "~")
}

function rankTextClassName(index: number): string {
  if (index === 0) return "font-bold text-[#f59e0b]"
  if (index === 1) return "font-bold text-[#9ca3af]"
  if (index === 2) return "font-bold text-[#b45309]"
  return "font-medium text-[#888]"
}
