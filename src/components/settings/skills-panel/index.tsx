import { useState, useEffect, useCallback, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs"
import { getTransport } from "@/lib/transport-provider"
import { isTauriMode } from "@/lib/transport"
import { logger } from "@/lib/logger"
import {
  markDraftsSeen,
  refreshDraftSkillsStore,
  useDraftSkillsStore,
} from "@/hooks/useDraftSkillsStore"
import { SKILLS_EVENTS } from "@/types/skills"
import ServerDirectoryBrowser from "@/components/chat/input/ServerDirectoryBrowser"
import { useDirectoryPicker } from "@/components/chat/input/useDirectoryPicker"
import type { SkillStatusEntry, SkillSummary } from "../types"
import type { SkillDetail } from "./types"
import {
  addSkillsDirectory,
  getSkillEnv,
  getSkillsEnvStatus,
  getSkillsStatus,
  loadSkillDetail,
  loadSkillMarketSources,
  loadSkillMarketSnapshot,
  loadSkillDockSnapshot,
  loadSkillRegistrySnapshot,
  reloadSkillsManagerSnapshot,
  loadSkillsManagerSnapshot,
  removeSkillEnvVar,
  saveSkillMarketSources,
  setSkillEnabled,
  setSkillEnvVar,
} from "./api"
import SkillListView from "./SkillListView"
import SkillEvolutionView from "./SkillEvolutionView"
import SkillDetailView from "./SkillDetailView"
import SkillDockExtensionsView from "./SkillDockExtensionsView"
import QuickImportDialog from "./QuickImportDialog"
import type { SkillDockSnapshot, SkillRegistrySnapshot, SkillRemoteMarketSnapshot } from "./types"

type SkillsPanelTab = "skills" | "market" | "evolution" | "settings"

export default function SkillsPanel() {
  const { t } = useTranslation()
  const { drafts } = useDraftSkillsStore()
  const [activeTab, setActiveTab] = useState<SkillsPanelTab>("skills")
  const [skills, setSkills] = useState<SkillSummary[]>([])
  const [search, setSearch] = useState("")
  const [sourceFilter, setSourceFilter] = useState<"all" | "bundled" | "user">("all")
  const [stateFilter, setStateFilter] = useState<"all" | "enabled" | "disabled" | "attention">(
    "all",
  )
  const [sortKey, setSortKey] = useState<"name" | "source" | "status">("name")
  const [draftPending, setDraftPending] = useState<
    Record<string, "activate" | "discard" | undefined>
  >({})
  const [extraDirs, setExtraDirs] = useState<string[]>([])
  const [selectedSkill, setSelectedSkill] = useState<SkillDetail | null>(null)
  const [loading, setLoading] = useState(true)
  const [quickImportOpen, setQuickImportOpen] = useState(false)
  const [autoReviewEnabled, setAutoReviewEnabled] = useState(true)
  const [autoReviewPromotion, setAutoReviewPromotion] = useState(false)
  // Per-skill env status: skill_name -> { env_var -> is_configured }
  const [envStatus, setEnvStatus] = useState<Record<string, Record<string, boolean>>>({})
  const [skillStatuses, setSkillStatuses] = useState<SkillStatusEntry[]>([])
  const [skillDockSnapshot, setSkillDockSnapshot] = useState<SkillDockSnapshot | null>(null)
  const [skillRegistrySnapshot, setSkillRegistrySnapshot] = useState<SkillRegistrySnapshot | null>(
    null,
  )
  const [skillMarketSnapshot, setSkillMarketSnapshot] = useState<SkillRemoteMarketSnapshot | null>(
    null,
  )
  const [skillMarketLoading, setSkillMarketLoading] = useState(false)
  const [skillMarketError, setSkillMarketError] = useState<string | null>(null)
  const [skillMarketSourceUrls, setSkillMarketSourceUrls] = useState<string[]>([])
  // Env var values for the currently selected skill detail (masked from backend)
  const [envValues, setEnvValues] = useState<Record<string, string>>({})
  // Tracks which env vars the user has edited (dirty state)
  const [envDirty, setEnvDirty] = useState<Record<string, boolean>>({})
  // Saving state per key
  const [envSaving, setEnvSaving] = useState<Record<string, boolean>>({})

  const reload = useCallback(async () => {
    try {
      const [snapshot, dockSnapshot, registrySnapshot] = await Promise.all([
        loadSkillsManagerSnapshot(),
        loadSkillDockSnapshot(),
        loadSkillRegistrySnapshot(),
      ])
      setSkills(snapshot.skills)
      setExtraDirs(snapshot.extraDirs)
      setEnvStatus(snapshot.envStatus)
      setSkillStatuses(snapshot.skillStatuses)
      setSkillDockSnapshot(dockSnapshot)
      setSkillRegistrySnapshot(registrySnapshot)
    } catch (e) {
      logger.error("settings", "SkillsPanel::load", "Failed to load skills", e)
    } finally {
      setLoading(false)
    }
  }, [])

  const loadMarket = useCallback(
    async (sourceUrls = skillMarketSourceUrls) => {
      try {
        setSkillMarketLoading(true)
        setSkillMarketError(null)
        setSkillMarketSnapshot(await loadSkillMarketSnapshot(sourceUrls))
      } catch (e) {
        logger.error("settings", "SkillsPanel::loadMarket", "Failed to load skill market", e)
        setSkillMarketError(e instanceof Error ? e.message : String(e))
      } finally {
        setSkillMarketLoading(false)
      }
    },
    [skillMarketSourceUrls],
  )

  const updateMarketSources = useCallback(
    async (sourceUrls: string[]) => {
      setSkillMarketSourceUrls(sourceUrls)
      try {
        const saved = await saveSkillMarketSources(sourceUrls)
        setSkillMarketSourceUrls(saved)
        await loadMarket(saved)
      } catch (e) {
        logger.error(
          "settings",
          "SkillsPanel::saveMarketSources",
          "Failed to save skill market sources",
          e,
        )
        setSkillMarketError(e instanceof Error ? e.message : String(e))
      }
    },
    [loadMarket],
  )

  const rescan = useCallback(async () => {
    try {
      setLoading(true)
      const [snapshot, dockSnapshot, registrySnapshot] = await Promise.all([
        reloadSkillsManagerSnapshot(),
        loadSkillDockSnapshot(),
        loadSkillRegistrySnapshot(),
      ])
      setSkills(snapshot.skills)
      setExtraDirs(snapshot.extraDirs)
      setEnvStatus(snapshot.envStatus)
      setSkillStatuses(snapshot.skillStatuses)
      setSkillDockSnapshot(dockSnapshot)
      setSkillRegistrySnapshot(registrySnapshot)
    } catch (e) {
      logger.error("settings", "SkillsPanel::rescan", "Failed to rescan skills", e)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    reload()
    const unlisten = getTransport().listen(SKILLS_EVENTS.autoReviewComplete, () => {
      reload()
    })
    return unlisten
  }, [reload])

  useEffect(() => {
    let cancelled = false
    loadSkillMarketSources()
      .then((sources) => {
        if (!cancelled) setSkillMarketSourceUrls(sources)
      })
      .catch((e) => {
        logger.error(
          "settings",
          "SkillsPanel::loadMarketSources",
          "Failed to load skill market sources",
          e,
        )
      })
    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    if (
      activeTab === "market" &&
      !skillMarketSnapshot &&
      !skillMarketLoading &&
      !skillMarketError
    ) {
      void loadMarket()
    }
  }, [activeTab, loadMarket, skillMarketError, skillMarketLoading, skillMarketSnapshot])

  // Drafts now live inside the Evolution tab — only mark them seen when the
  // user actually lands on (or is already on) that tab. Doing it on panel
  // mount would clear the IconSidebar / SettingsView dots before the user
  // ever sees the list.
  useEffect(() => {
    if (activeTab === "evolution") {
      markDraftsSeen()
    }
  }, [activeTab, drafts])

  useEffect(() => {
    let cancelled = false
    Promise.all([
      getTransport().call<boolean>("get_skills_auto_review_enabled"),
      getTransport().call<boolean>("get_skills_auto_review_promotion"),
    ])
      .then(([enabled, promotion]) => {
        if (cancelled) return
        setAutoReviewEnabled(enabled)
        setAutoReviewPromotion(promotion)
      })
      .catch((e) => {
        logger.error(
          "settings",
          "SkillsPanel::loadAutoReview",
          "Failed to load auto-review settings",
          e,
        )
      })
    return () => {
      cancelled = true
    }
  }, [])

  const draftNames = useMemo(() => new Set(drafts.map((d) => d.name)), [drafts])
  const visibleSkills = useMemo(
    () => skills.filter((s) => !draftNames.has(s.name)),
    [skills, draftNames],
  )
  const skillStatusByName = useMemo(() => {
    const next: Record<string, SkillStatusEntry | undefined> = {}
    for (const status of skillStatuses) next[status.name] = status
    return next
  }, [skillStatuses])

  async function handleActivateDraft(name: string) {
    setDraftPending((prev) => ({ ...prev, [name]: "activate" }))
    try {
      await getTransport().call("activate_draft_skill", { name })
      await reload()
      refreshDraftSkillsStore()
    } catch (e) {
      logger.error("settings", "SkillsPanel::activateDraft", "Failed to activate", e)
    } finally {
      setDraftPending((prev) => ({ ...prev, [name]: undefined }))
    }
  }

  async function handleDiscardDraft(name: string) {
    setDraftPending((prev) => ({ ...prev, [name]: "discard" }))
    try {
      await getTransport().call("discard_draft_skill", { name })
      await reload()
      refreshDraftSkillsStore()
    } catch (e) {
      logger.error("settings", "SkillsPanel::discardDraft", "Failed to discard", e)
    } finally {
      setDraftPending((prev) => ({ ...prev, [name]: undefined }))
    }
  }

  async function handleOpenDir(path: string) {
    try {
      await getTransport().call("open_directory", { path })
    } catch (e) {
      logger.error("settings", "SkillsPanel::openDir", "Failed to open directory", e)
    }
  }

  const addExtraDir = useCallback(
    async (dir: string) => {
      try {
        await addSkillsDirectory(dir)
        await reload()
      } catch (e) {
        logger.error("settings", "SkillsPanel::addDir", "Failed to add skills directory", e)
      }
    },
    [reload],
  )

  const {
    pick: handleAddDir,
    browserOpen: dirBrowserOpen,
    setBrowserOpen: setDirBrowserOpen,
    handleBrowserSelect: handleDirBrowserSelect,
  } = useDirectoryPicker({
    onPicked: (path) => {
      void addExtraDir(path)
    },
    errorTitle: t("settings.skillsDirPickFailed"),
    loggerSource: "SkillsPanel::pickExtraDir",
  })

  async function handleToggleSkill(name: string, enabled: boolean) {
    try {
      await setSkillEnabled(name, enabled)
      // Update local state immediately
      setSkills((prev) => prev.map((s) => (s.name === name ? { ...s, enabled } : s)))
      setSkillStatuses((prev) =>
        prev.map((s) =>
          s.name === name
            ? {
                ...s,
                disabled: !enabled,
                eligible: enabled && !s.blocked_by_allowlist && !s.hard_blocked && !s.needs_setup,
              }
            : s,
        ),
      )
      if (selectedSkill?.name === name) {
        setSelectedSkill((prev) => (prev ? { ...prev, enabled } : prev))
      }
    } catch (e) {
      logger.error("settings", "SkillsPanel::toggle", "Failed to toggle skill", e)
    }
  }

  async function handleSelectSkill(name: string) {
    try {
      const { detail, maskedEnv } = await loadSkillDetail(name)
      setSelectedSkill(detail)
      setEnvValues(maskedEnv)
      setEnvDirty({})
      setEnvSaving({})
    } catch (e) {
      logger.error("settings", "SkillsPanel::detail", "Failed to load skill detail", e)
    }
  }

  async function handleSaveEnvVar(key: string) {
    if (!selectedSkill) return
    const value = envValues[key] ?? ""
    setEnvSaving((prev) => ({ ...prev, [key]: true }))
    try {
      await setSkillEnvVar(selectedSkill.name, key, value)
      // Re-fetch the masked value
      const maskedEnv = await getSkillEnv(selectedSkill.name)
      setEnvValues(maskedEnv)
      setEnvDirty((prev) => ({ ...prev, [key]: false }))
      // Refresh env status
      const [nextEnvStatus, nextSkillStatuses] = await Promise.all([
        getSkillsEnvStatus(),
        getSkillsStatus(),
      ])
      setEnvStatus(nextEnvStatus)
      setSkillStatuses(nextSkillStatuses)
    } catch (e) {
      logger.error("settings", "SkillsPanel::saveEnv", "Failed to save env var", e)
    } finally {
      setEnvSaving((prev) => ({ ...prev, [key]: false }))
    }
  }

  async function handleRemoveEnvVar(key: string) {
    if (!selectedSkill) return
    try {
      await removeSkillEnvVar(selectedSkill.name, key)
      setEnvValues((prev) => {
        const next = { ...prev }
        delete next[key]
        return next
      })
      setEnvDirty((prev) => ({ ...prev, [key]: false }))
      // Refresh env status
      const [nextEnvStatus, nextSkillStatuses] = await Promise.all([
        getSkillsEnvStatus(),
        getSkillsStatus(),
      ])
      setEnvStatus(nextEnvStatus)
      setSkillStatuses(nextSkillStatuses)
    } catch (e) {
      logger.error("settings", "SkillsPanel::removeEnv", "Failed to remove env var", e)
    }
  }

  function handleEnvValueChange(key: string, value: string) {
    setEnvValues((prev) => ({ ...prev, [key]: value }))
    setEnvDirty((prev) => ({ ...prev, [key]: true }))
  }

  async function handleSetAutoReviewPromotion(v: boolean) {
    const previous = autoReviewPromotion
    setAutoReviewPromotion(v)
    try {
      await getTransport().call("set_skills_auto_review_promotion", { auto: v })
    } catch (e) {
      logger.error(
        "settings",
        "SkillsPanel::setAutoReviewPromotion",
        "Failed to update auto-review promotion",
        e,
      )
      setAutoReviewPromotion(previous)
    }
  }

  async function handleSetAutoReviewEnabled(v: boolean) {
    const previous = autoReviewEnabled
    setAutoReviewEnabled(v)
    try {
      await getTransport().call("set_skills_auto_review_enabled", { enabled: v })
    } catch (e) {
      logger.error(
        "settings",
        "SkillsPanel::setAutoReviewEnabled",
        "Failed to update auto-review enabled",
        e,
      )
      setAutoReviewEnabled(previous)
    }
  }

  // ── Skill Detail View ──────────────────────────────────────────
  if (selectedSkill) {
    return (
      <SkillDetailView
        skill={selectedSkill}
        envStatus={envStatus}
        status={skillStatusByName[selectedSkill.name]}
        dockSnapshot={skillDockSnapshot}
        envValues={envValues}
        envDirty={envDirty}
        envSaving={envSaving}
        onBack={() => setSelectedSkill(null)}
        onToggleSkill={handleToggleSkill}
        onOpenDir={handleOpenDir}
        onEnvValueChange={handleEnvValueChange}
        onSaveEnvVar={handleSaveEnvVar}
        onRemoveEnvVar={handleRemoveEnvVar}
      />
    )
  }

  // ── Skills List View ───────────────────────────────────────────
  return (
    <div className="flex-1 min-h-0 overflow-hidden flex flex-col">
      <Tabs
        value={activeTab}
        onValueChange={(v) => setActiveTab(v as SkillsPanelTab)}
        className="flex-1 flex flex-col min-h-0"
      >
        <div className="px-6 pt-4 shrink-0">
          <TabsList>
            <TabsTrigger value="skills">{t("settings.skillsTab.skills")}</TabsTrigger>
            <TabsTrigger value="market">{t("settings.skillsTab.market")}</TabsTrigger>
            <TabsTrigger value="evolution" className="gap-1.5">
              {t("settings.skillsTab.evolution")}
              {drafts.length > 0 && (
                <span className="inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-full bg-amber-500/15 px-1.5 text-[10px] font-semibold text-amber-600 dark:text-amber-400">
                  {drafts.length}
                </span>
              )}
            </TabsTrigger>
            <TabsTrigger value="settings">{t("settings.skillsTab.settings")}</TabsTrigger>
          </TabsList>
        </div>
        <TabsContent value="skills" className="flex-1 min-h-0 outline-none">
          <SkillDockExtensionsView
            skills={visibleSkills}
            extraDirs={extraDirs}
            localSkillsContent={
              <SkillListView
                skills={visibleSkills}
                loading={loading}
                envStatus={envStatus}
                skillStatusByName={skillStatusByName}
                dockSnapshot={skillDockSnapshot}
                onToggleSkill={handleToggleSkill}
                onSelectSkill={handleSelectSkill}
                onOpenDir={handleOpenDir}
                onAddDir={handleAddDir}
                search={search}
                sourceFilter={sourceFilter}
                stateFilter={stateFilter}
                sortKey={sortKey}
                onSearchChange={setSearch}
                onSourceFilterChange={setSourceFilter}
                onStateFilterChange={setStateFilter}
                onSortKeyChange={setSortKey}
                onReload={() => void rescan()}
              />
            }
            initialSection="overview"
            snapshot={skillDockSnapshot}
            registry={skillRegistrySnapshot}
            market={skillMarketSnapshot}
            marketLoading={skillMarketLoading}
            marketError={skillMarketError}
            marketSourceUrls={skillMarketSourceUrls}
            onMarketSourceUrlsChange={(urls) => void updateMarketSources(urls)}
            onRefreshMarket={() => void loadMarket()}
            onQuickImport={() => setQuickImportOpen(true)}
            onAddDir={handleAddDir}
            onImported={reload}
            onRefresh={reload}
          />
        </TabsContent>
        <TabsContent value="market" className="flex-1 min-h-0 outline-none">
          <SkillDockExtensionsView
            skills={visibleSkills}
            extraDirs={extraDirs}
            initialSection="market"
            lockedSection="market"
            snapshot={skillDockSnapshot}
            registry={skillRegistrySnapshot}
            market={skillMarketSnapshot}
            marketLoading={skillMarketLoading}
            marketError={skillMarketError}
            marketSourceUrls={skillMarketSourceUrls}
            onMarketSourceUrlsChange={(urls) => void updateMarketSources(urls)}
            onRefreshMarket={() => void loadMarket()}
            onQuickImport={() => setQuickImportOpen(true)}
            onAddDir={handleAddDir}
            onImported={reload}
            onRefresh={reload}
          />
        </TabsContent>
        <TabsContent value="settings" className="flex-1 min-h-0 outline-none">
          <SkillDockExtensionsView
            skills={visibleSkills}
            extraDirs={extraDirs}
            initialSection="settings"
            lockedSection="settings"
            snapshot={skillDockSnapshot}
            registry={skillRegistrySnapshot}
            market={skillMarketSnapshot}
            marketLoading={skillMarketLoading}
            marketError={skillMarketError}
            marketSourceUrls={skillMarketSourceUrls}
            onMarketSourceUrlsChange={(urls) => void updateMarketSources(urls)}
            onRefreshMarket={() => void loadMarket()}
            onQuickImport={() => setQuickImportOpen(true)}
            onAddDir={handleAddDir}
            onImported={reload}
            onRefresh={reload}
          />
        </TabsContent>
        <TabsContent value="evolution" className="flex-1 min-h-0 outline-none">
          <SkillEvolutionView
            autoReviewEnabled={autoReviewEnabled}
            autoReviewPromotion={autoReviewPromotion}
            onSetAutoReviewEnabled={handleSetAutoReviewEnabled}
            onSetAutoReviewPromotion={handleSetAutoReviewPromotion}
            drafts={drafts}
            draftPending={draftPending}
            onActivateDraft={handleActivateDraft}
            onDiscardDraft={handleDiscardDraft}
            onSelectSkill={handleSelectSkill}
          />
        </TabsContent>
      </Tabs>
      <QuickImportDialog
        open={quickImportOpen}
        onClose={() => setQuickImportOpen(false)}
        onImported={reload}
      />
      {!isTauriMode() && (
        <ServerDirectoryBrowser
          open={dirBrowserOpen}
          onOpenChange={setDirBrowserOpen}
          onSelect={handleDirBrowserSelect}
        />
      )}
    </div>
  )
}
