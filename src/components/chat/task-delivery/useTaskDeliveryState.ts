import { useEffect, useMemo, useRef, useState } from "react"
import { getTransport } from "@/lib/transport-provider"
import { parsePayload } from "@/lib/transport"
import type { ProjectMeta, ProjectWorkflowDiscovery, ProjectWorkflowPreview } from "@/types/project"
import type { TaskProgressSnapshot } from "@/components/chat/tasks/taskProgress"
import {
  buildTaskDeliveryState,
  type TaskDeliveryInstanceData,
  type TaskDeliveryProgressJson,
  type TaskDeliveryState,
  type TaskDeliveryTaskCandidate,
} from "./taskDelivery"

interface WorkspaceEntry {
  name: string
  relPath: string
  isDir: boolean
  size?: number | null
  modifiedMs?: number | null
}

interface WorkspaceListing {
  dirRel: string
  entries: WorkspaceEntry[]
  truncated?: boolean
}

interface FileTextContent {
  content: string
  isBinary?: boolean
}

interface ProjectFsChangedPayload {
  scope?: string
  scopeId?: string
  dir?: string
  path?: string
}

function projectArgs(projectId: string, path?: string) {
  return { scope: "project", scopeId: projectId, scope_id: projectId, path }
}

async function listProjectDir(projectId: string, path: string): Promise<WorkspaceListing | null> {
  try {
    return await getTransport().call<WorkspaceListing>("project_fs_list", projectArgs(projectId, path))
  } catch {
    return null
  }
}

async function readProjectText(projectId: string, path: string): Promise<string | null> {
  try {
    const result = await getTransport().call<FileTextContent>("project_fs_read_text", projectArgs(projectId, path))
    return result.isBinary ? null : result.content
  } catch {
    return null
  }
}

function sortRecentDirs(entries: WorkspaceEntry[]): WorkspaceEntry[] {
  return entries
    .filter((entry) => entry.isDir)
    .sort((a, b) => (b.modifiedMs ?? 0) - (a.modifiedMs ?? 0) || b.name.localeCompare(a.name))
}

function shouldReadArtifactContent(entry: WorkspaceEntry): boolean {
  if (entry.isDir) return false
  if (!entry.name.endsWith(".md")) return false
  return (entry.size ?? 0) <= 256 * 1024
}

function shouldRefreshForPath(path?: string | null): boolean {
  if (!path) return false
  const normalized = path.replace(/\\/g, "/")
  return normalized.startsWith("docs/tasks/") || normalized.startsWith(".agent-workflows/")
}

async function readTaskInstance(projectId: string, taskDirRel: string): Promise<TaskDeliveryInstanceData | null> {
  const taskListing = await listProjectDir(projectId, taskDirRel)
  if (!taskListing) return null
  const progressEntry = taskListing.entries.find((entry) => !entry.isDir && entry.name === "progress.json")
  if (!progressEntry) return null

  const text = await readProjectText(projectId, progressEntry.relPath)
  let progress: TaskDeliveryProgressJson | null = null
  if (text) {
    try {
      progress = JSON.parse(text) as TaskDeliveryProgressJson
    } catch {
      progress = null
    }
  }

  const artifactEntries = new Map(
    taskListing.entries
      .filter((entry) => !entry.isDir)
      .map((entry) => [
        entry.name,
        { sizeBytes: entry.size, modifiedMs: entry.modifiedMs },
      ]),
  )
  const artifactContents = new Map<string, string>()
  const readableArtifacts = taskListing.entries.filter(shouldReadArtifactContent)
  await Promise.all(
    readableArtifacts.map(async (entry) => {
      const content = await readProjectText(projectId, entry.relPath)
      if (content !== null) artifactContents.set(entry.name, content)
    }),
  )

  return {
    taskDir: taskDirRel,
    progress,
    artifactEntries,
    artifactContents,
  }
}

function candidateFromInstance(instance: TaskDeliveryInstanceData, modifiedMs?: number | null): TaskDeliveryTaskCandidate {
  const progress = instance.progress
  const label = progress?.task_title || progress?.task_id || instance.taskDir.split(/[\\/]/).pop() || instance.taskDir
  return {
    taskDir: instance.taskDir,
    label,
    taskId: progress?.task_id,
    taskTitle: progress?.task_title,
    status: progress?.status,
    currentPhaseId: progress?.current_phase_id,
    updatedAt: progress?.updated_at,
    modifiedMs,
  }
}

async function discoverTaskInstances(projectId: string): Promise<{ candidates: TaskDeliveryTaskCandidate[]; instances: TaskDeliveryInstanceData[] }> {
  const tasksRoot = await listProjectDir(projectId, "docs/tasks")
  if (!tasksRoot) return { candidates: [], instances: [] }

  const candidates: TaskDeliveryTaskCandidate[] = []
  const instances: TaskDeliveryInstanceData[] = []
  const monthDirs = sortRecentDirs(tasksRoot.entries).slice(0, 12)
  for (const monthDir of monthDirs) {
    const monthListing = await listProjectDir(projectId, monthDir.relPath)
    if (!monthListing) continue

    const taskDirs = sortRecentDirs(monthListing.entries).slice(0, 30)
    for (const taskDir of taskDirs) {
      const instance = await readTaskInstance(projectId, taskDir.relPath)
      if (!instance) continue
      instances.push(instance)
      candidates.push(candidateFromInstance(instance, taskDir.modifiedMs))
    }
  }

  return { candidates, instances }
}

interface UseTaskDeliveryStateInput {
  project?: ProjectMeta | null
  taskSnapshot?: TaskProgressSnapshot | null
}

export interface UseTaskDeliveryStateResult {
  state: TaskDeliveryState
  loading: boolean
  error: string | null
  refresh: () => void
  taskCandidates: TaskDeliveryTaskCandidate[]
  selectedTaskDir: string | null
  selectTaskDir: (taskDir: string | null) => void
}

export function useTaskDeliveryState({
  project,
  taskSnapshot,
}: UseTaskDeliveryStateInput): UseTaskDeliveryStateResult {
  const [discovery, setDiscovery] = useState<ProjectWorkflowDiscovery | null>(null)
  const [preview, setPreview] = useState<ProjectWorkflowPreview | null>(null)
  const [instance, setInstance] = useState<TaskDeliveryInstanceData | null>(null)
  const [instances, setInstances] = useState<TaskDeliveryInstanceData[]>([])
  const [taskCandidates, setTaskCandidates] = useState<TaskDeliveryTaskCandidate[]>([])
  const [selectedTaskDir, setSelectedTaskDir] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [refreshNonce, setRefreshNonce] = useState(0)
  const requestSeq = useRef(0)
  const autoRefreshTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const scheduleRefresh = () => {
    if (autoRefreshTimer.current) clearTimeout(autoRefreshTimer.current)
    autoRefreshTimer.current = setTimeout(() => {
      autoRefreshTimer.current = null
      setRefreshNonce((value) => value + 1)
    }, 350)
  }

  useEffect(() => {
    const projectId = project?.id
    if (!projectId) {
      requestSeq.current += 1
      setDiscovery(null)
      setPreview(null)
      setInstance(null)
      setInstances([])
      setTaskCandidates([])
      setSelectedTaskDir(null)
      setLoading(false)
      setError(null)
      return
    }

    const activeProjectId = projectId
    const seq = ++requestSeq.current
    setLoading(true)
    setError(null)
    setDiscovery(null)
    setPreview(null)
    setInstance(null)
    setInstances([])
    setTaskCandidates([])

    async function load() {
      try {
        const nextDiscovery = await getTransport().call<ProjectWorkflowDiscovery>(
          "discover_project_workflows_cmd",
          { id: activeProjectId },
        )
        if (seq !== requestSeq.current) return
        setDiscovery(nextDiscovery)

        const discovered = await discoverTaskInstances(activeProjectId)
        if (seq !== requestSeq.current) return
        setInstances(discovered.instances)
        setTaskCandidates(discovered.candidates)
        const nextInstance =
          discovered.instances.find((item) => item.taskDir === selectedTaskDir) ?? discovered.instances[0] ?? null
        setInstance(nextInstance)

        const template = nextDiscovery.templates[0]
        if (!nextDiscovery.exists || !template) return

        const nextPreview = await getTransport().call<ProjectWorkflowPreview>(
          "preview_project_workflow_cmd",
          {
            projectId: activeProjectId,
            input: {
              projectId: activeProjectId,
              templateId: template.id,
              mode: template.modes[0] ?? null,
              taskType: template.taskTypes[0] ?? null,
            },
          },
        )
        if (seq !== requestSeq.current) return
        setPreview(nextPreview)
      } catch (err) {
        if (seq !== requestSeq.current) return
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        if (seq === requestSeq.current) setLoading(false)
      }
    }

    void load()
    return () => {
      requestSeq.current += 1
    }
  }, [project?.id, refreshNonce, selectedTaskDir])

  useEffect(() => {
    const projectId = project?.id
    if (!projectId) return

    const unlisten = getTransport().listen("project:fs_changed", (raw) => {
      const payload = parsePayload<ProjectFsChangedPayload>(raw)
      if (payload?.scope !== "project" || payload.scopeId !== projectId) return
      if (shouldRefreshForPath(payload.path) || shouldRefreshForPath(payload.dir)) scheduleRefresh()
    })

    const poll = window.setInterval(() => {
      scheduleRefresh()
    }, 30_000)

    return () => {
      try {
        unlisten?.()
      } catch {
        // ignore stale listener cleanup
      }
      window.clearInterval(poll)
      if (autoRefreshTimer.current) {
        clearTimeout(autoRefreshTimer.current)
        autoRefreshTimer.current = null
      }
    }
  }, [project?.id])

  const state = useMemo(
    () => buildTaskDeliveryState({ project, discovery, preview, loading, error, taskSnapshot, instance }),
    [discovery, error, instance, loading, preview, project, taskSnapshot],
  )

  return {
    state,
    loading,
    error,
    refresh: () => setRefreshNonce((value) => value + 1),
    taskCandidates,
    selectedTaskDir: instance?.taskDir ?? selectedTaskDir,
    selectTaskDir: (taskDir) => {
      setSelectedTaskDir(taskDir)
      const selectedInstance = instances.find((item) => item.taskDir === taskDir) ?? null
      if (selectedInstance) setInstance(selectedInstance)
    },
  }
}
