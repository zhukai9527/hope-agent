import { useMemo, useState, type ReactNode } from "react"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import {
  Check,
  ChevronDown,
  GitBranch,
  GitCommitHorizontal,
  GitCompare,
  GitPullRequest,
  HardDrive,
  Loader2,
  Plus,
  Search,
  Trees,
  type LucideIcon,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { Textarea } from "@/components/ui/textarea"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { cn } from "@/lib/utils"
import { openExternalUrl } from "@/lib/openExternalUrl"
import type {
  GitMutationResult,
  GitPullRequestPreflight,
  ManagedWorktree,
  SessionGitDiffSnapshot,
} from "@/lib/transport"
import { getTransport } from "@/lib/transport-provider"
import { FileDeltaCounter } from "@/components/chat/message/FileDeltaCounter"
import type { SessionGitControlState } from "./useSessionGitControl"

interface GitControlCardProps {
  sessionId: string
  state: SessionGitControlState
  managedWorktrees: ManagedWorktree[]
  onOpenGitDiff: (snapshot: SessionGitDiffSnapshot, sessionId: string) => void
}

export function GitControlCard({
  sessionId,
  state,
  managedWorktrees,
  onOpenGitDiff,
}: GitControlCardProps) {
  const { t } = useTranslation()
  const snapshot = state.snapshot
  const [action, setAction] = useState<string | null>(null)
  const [branchOpen, setBranchOpen] = useState(false)
  const [branchQuery, setBranchQuery] = useState("")
  const [newBranch, setNewBranch] = useState("")
  const [commitOpen, setCommitOpen] = useState(false)
  const [commitSubject, setCommitSubject] = useState("")
  const [commitBody, setCommitBody] = useState("")
  const [stageAll, setStageAll] = useState(false)
  const [pushAfter, setPushAfter] = useState(false)
  const [prOpen, setPrOpen] = useState(false)
  const [prTitle, setPrTitle] = useState("")
  const [prBody, setPrBody] = useState("")
  const [prBase, setPrBase] = useState("")
  const [prDraft, setPrDraft] = useState(true)
  const [prPushFirst, setPrPushFirst] = useState(true)
  const [prPreflight, setPrPreflight] = useState<GitPullRequestPreflight | null>(null)

  const branches = useMemo(() => {
    const query = branchQuery.trim().toLowerCase()
    if (!snapshot) return []
    return query
      ? snapshot.branches.filter((branch) => branch.name.toLowerCase().includes(query))
      : snapshot.branches
  }, [branchQuery, snapshot])
  const branchGroups = useMemo(
    () => [
      {
        kind: "local" as const,
        label: t("workspace.git.localBranches", "本地分支"),
        branches: branches.filter((branch) => branch.kind === "local"),
      },
      {
        kind: "remote" as const,
        label: t("workspace.git.remoteBranches", "远端跟踪分支"),
        branches: branches.filter((branch) => branch.kind === "remote"),
      },
    ],
    [branches, t],
  )

  const run = async <T,>(key: string, task: () => Promise<T>): Promise<T | null> => {
    if (action) return null
    setAction(key)
    try {
      const value = await task()
      state.refresh()
      return value
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error))
      return null
    } finally {
      setAction(null)
    }
  }

  const openChanges = () =>
    void run("diff", async () => {
      const diff = await getTransport().call<SessionGitDiffSnapshot>(
        "load_session_git_diff_snapshot_cmd",
        { sessionId, scope: "unstaged" },
      )
      onOpenGitDiff(diff, sessionId)
      return diff
    })

  const switchBranch = (fullRef: string) =>
    void run("branch", async () => {
      if (!snapshot) throw new Error("Git snapshot unavailable")
      const result = await getTransport().call<GitMutationResult>(
        "switch_session_git_branch_cmd",
        {
          sessionId,
          input: {
            requestId: crypto.randomUUID(),
            expectedRevision: snapshot.revision,
            fullRef,
          },
        },
      )
      setBranchOpen(false)
      toast.success(t("workspace.git.branchSwitched", "分支已切换"))
      return result
    })

  const createBranch = () =>
    void run("branch", async () => {
      if (!snapshot || !newBranch.trim()) throw new Error("请输入分支名")
      const result = await getTransport().call<GitMutationResult>(
        "create_session_git_branch_cmd",
        {
          sessionId,
          input: {
            requestId: crypto.randomUUID(),
            expectedRevision: snapshot.revision,
            name: newBranch.trim(),
          },
        },
      )
      setNewBranch("")
      setBranchOpen(false)
      toast.success(t("workspace.git.branchCreated", "分支已创建"))
      return result
    })

  const handoff = (target: "local" | "worktree", worktreeId?: string) =>
    void run("handoff", async () => {
      if (!snapshot) throw new Error("Git snapshot unavailable")
      const result = await getTransport().call<GitMutationResult>("handoff_session_git_cmd", {
        sessionId,
        input: {
          requestId: crypto.randomUUID(),
          expectedRevision: snapshot.revision,
          target,
          worktreeId,
        },
      })
      toast.success(t("workspace.git.handoffDone", "运行位置已安全交接"))
      return result
    })

  const submitCommit = () =>
    void run("commit", async () => {
      if (!snapshot) throw new Error("Git snapshot unavailable")
      const result = await getTransport().call<GitMutationResult>("commit_session_git_cmd", {
        sessionId,
        input: {
          requestId: crypto.randomUUID(),
          expectedRevision: snapshot.revision,
          subject: commitSubject,
          body: commitBody || null,
          stageAll,
          pushAfter,
          remote: snapshot.remotes.find((remote) => remote.isDefault)?.name ?? null,
        },
      })
      setCommitOpen(false)
      setCommitSubject("")
      setCommitBody("")
      if (result.warning) toast.warning(`${result.message}: ${result.warning}`)
      else toast.success(result.message)
      return result
    })

  const push = () =>
    void run("push", async () => {
      if (!snapshot) throw new Error("Git snapshot unavailable")
      const result = await getTransport().call<GitMutationResult>("push_session_git_cmd", {
        sessionId,
        input: {
          requestId: crypto.randomUUID(),
          expectedRevision: snapshot.revision,
          remote: snapshot.remotes.find((remote) => remote.isDefault)?.name ?? null,
          setUpstream: true,
        },
      })
      toast.success(result.message)
      return result
    })

  const openPullRequest = () =>
    void run("pr-preflight", async () => {
      const preflight = await getTransport().call<GitPullRequestPreflight>(
        "session_git_pr_preflight_cmd",
        { sessionId },
      )
      setPrPreflight(preflight)
      if (preflight.current?.url) {
        openExternalUrl(preflight.current.url)
        return preflight
      }
      if (!preflight.available) throw new Error(preflight.errorMessage || "Pull Request 不可用")
      setPrBase(preflight.defaultBranch || "main")
      setPrTitle(commitSubject || snapshot?.branch || "")
      setPrOpen(true)
      return preflight
    })

  const submitPullRequest = () =>
    void run("pr-create", async () => {
      if (!snapshot) throw new Error("Git snapshot unavailable")
      const result = await getTransport().call<GitMutationResult>("create_session_git_pr_cmd", {
        sessionId,
        input: {
          requestId: crypto.randomUUID(),
          expectedRevision: snapshot.revision,
          title: prTitle,
          body: prBody || null,
          baseBranch: prBase,
          draft: prDraft,
          pushFirst: prPushFirst,
          remote: snapshot.remotes.find((remote) => remote.isDefault)?.name ?? null,
        },
      })
      setPrOpen(false)
      if (result.url) openExternalUrl(result.url)
      toast.success(result.message)
      return result
    })

  if (state.loading && !snapshot) {
    return <div className="flex h-28 items-center justify-center"><Loader2 className="h-4 w-4 animate-spin text-muted-foreground" /></div>
  }
  if (!snapshot) return null

  const dirty = snapshot.dirty.changedFiles > 0
  const ahead = snapshot.sync.ahead
  const busy = Boolean(action)

  return (
    <>
      <div className="overflow-hidden rounded-xl border border-border/65 bg-secondary/10 p-1">
        <GitRow
          icon={GitCompare}
          label={t("workspace.git.changes", "变更")}
          onClick={openChanges}
          disabled={!dirty || busy}
          value={dirty ? `${snapshot.dirty.changedFiles}` : t("workspace.git.clean", "无变更")}
          trailing={(snapshot.status.linesAdded > 0 || snapshot.status.linesRemoved > 0) ? <FileDeltaCounter linesAdded={snapshot.status.linesAdded} linesRemoved={snapshot.status.linesRemoved} /> : null}
          loading={action === "diff"}
        />

        <DropdownMenu>
          <DropdownMenuTrigger
            className="flex h-9 w-full items-center gap-2 rounded-lg px-2.5 text-left transition-colors hover:bg-secondary disabled:cursor-not-allowed disabled:opacity-45"
            disabled={busy || !snapshot.capabilities.canHandoff}
          >
            {snapshot.activeLocation === "local" ? (
              <HardDrive className="h-4 w-4 shrink-0" />
            ) : (
              <Trees className="h-4 w-4 shrink-0" />
            )}
            <span className="min-w-0 flex-1 truncate text-sm font-medium">
              {t("workspace.git.location", "运行位置")}
            </span>
            <span className="max-w-[45%] truncate text-xs text-muted-foreground">
              {snapshot.activeLocation === "local"
                ? t("workspace.git.local", "本地")
                : t("workspace.git.worktree", "工作树")}
            </span>
            {action === "handoff" ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <ChevronDown className="h-3.5 w-3.5" />
            )}
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="w-64">
            <DropdownMenuItem disabled={snapshot.activeLocation === "local"} onSelect={() => handoff("local")}>
              <HardDrive className="mr-2 h-4 w-4" />
              {t("workspace.git.localCheckout", "本地检出")}
              {snapshot.activeLocation === "local" ? <Check className="ml-auto h-4 w-4" /> : null}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            {managedWorktrees.filter((worktree) => worktree.pathExists && worktree.state !== "archived").map((worktree) => (
              <DropdownMenuItem key={worktree.id} disabled={snapshot.managedWorktreeId === worktree.id} onSelect={() => handoff("worktree", worktree.id)}>
                <Trees className="mr-2 h-4 w-4" />
                <span className="truncate">{worktree.label || worktree.id}</span>
                {snapshot.managedWorktreeId === worktree.id ? <Check className="ml-auto h-4 w-4" /> : null}
              </DropdownMenuItem>
            ))}
            <DropdownMenuItem onSelect={() => handoff("worktree")}>
              <Plus className="mr-2 h-4 w-4" />
              {t("workspace.git.newWorktree", "新建工作树")}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>

        <GitRow
          icon={GitBranch}
          label={snapshot.detached ? t("workspace.git.createBranch", "创建分支") : t("workspace.git.branch", "分支")}
          value={snapshot.branch || `HEAD ${snapshot.head?.slice(0, 8) || ""}`}
          trailing={<ChevronDown className="h-3.5 w-3.5" />}
          onClick={() => setBranchOpen(true)}
          disabled={busy || !snapshot.capabilities.canCreateBranch}
          loading={action === "branch"}
        />

        <GitRow
          icon={GitCommitHorizontal}
          label={dirty ? t("workspace.git.commit", "提交") : t("workspace.git.push", "推送")}
          value={!dirty && ahead > 0 ? t("workspace.git.aheadCount", "{{count}} 个提交", { count: ahead }) : undefined}
          onClick={() => dirty ? setCommitOpen(true) : push()}
          disabled={busy || snapshot.detached || (!dirty && (!snapshot.capabilities.canPush || ahead === 0))}
          loading={action === "commit" || action === "push"}
        />

        <GitRow
          icon={GitPullRequest}
          label={prPreflight?.current ? t("workspace.git.viewPr", "查看拉取请求") : t("workspace.git.createPr", "创建拉取请求")}
          onClick={openPullRequest}
          disabled={busy || !snapshot.capabilities.canCreatePullRequest}
          loading={action === "pr-preflight" || action === "pr-create"}
        />
      </div>
      {state.progress ? (
        <div className="mt-1.5 flex items-center gap-2 px-2 text-[11px] text-muted-foreground">
          <Loader2 className="h-3 w-3 animate-spin" />
          <span className="truncate">
            {state.progress.message || state.progress.stage}
          </span>
        </div>
      ) : null}

      <Dialog open={branchOpen} onOpenChange={setBranchOpen}>
        <DialogContent className="max-w-md">
          <DialogHeader><DialogTitle>{t("workspace.git.selectBranch", "选择分支")}</DialogTitle><DialogDescription>{dirty ? t("workspace.git.dirtySwitchBlocked", "当前有未提交改动，只能创建新分支；切换其他分支前请先处理改动。") : t("workspace.git.branchHint", "选择本地或远端跟踪分支。")}</DialogDescription></DialogHeader>
          <div className="relative"><Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" /><Input value={branchQuery} onChange={(event) => setBranchQuery(event.target.value)} className="pl-8" placeholder={t("workspace.git.searchBranches", "搜索分支")} /></div>
          <div className="max-h-64 space-y-1 overflow-auto rounded-md border p-1">
            {branchGroups.map((group) =>
              group.branches.length > 0 ? (
                <div key={group.kind}>
                  <div className="px-2 py-1 text-[11px] font-medium text-muted-foreground">
                    {group.label}
                  </div>
                  {group.branches.map((branch) => (
                    <button key={branch.fullRef} type="button" title={branch.isCheckedOut && !branch.isCurrent ? t("workspace.git.checkedOutAt", "已在 {{path}} 检出", { path: branch.checkedOutPath || t("workspace.git.anotherWorktree", "其他工作树") }) : undefined} className={cn("flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm hover:bg-secondary", (branch.isCurrent || branch.isCheckedOut || dirty) && "opacity-50")} disabled={branch.isCurrent || branch.isCheckedOut || dirty || busy} onClick={() => switchBranch(branch.fullRef)}>
                      <GitBranch className="h-3.5 w-3.5" /><span className="min-w-0 flex-1 truncate">{branch.name}</span>{branch.isCurrent ? <Check className="h-4 w-4" /> : null}
                    </button>
                  ))}
                </div>
              ) : null,
            )}
          </div>
          <div className="flex gap-2 border-t pt-3"><Input value={newBranch} onChange={(event) => setNewBranch(event.target.value)} placeholder="hope-agent/feature-name" /><Button onClick={createBranch} disabled={!newBranch.trim() || busy}>{t("workspace.git.create", "创建")}</Button></div>
        </DialogContent>
      </Dialog>

      <Dialog open={commitOpen} onOpenChange={setCommitOpen}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("workspace.git.commitTitle", "提交变更")}</DialogTitle><DialogDescription>{t("workspace.git.commitHint", "默认只提交已暂存内容；可选择同时暂存当前全部变更。")}</DialogDescription></DialogHeader>
          <Input value={commitSubject} onChange={(event) => setCommitSubject(event.target.value)} placeholder={t("workspace.git.commitSubject", "提交标题")} />
          <Textarea value={commitBody} onChange={(event) => setCommitBody(event.target.value)} placeholder={t("workspace.git.commitBody", "说明（可选）")} />
          <ToggleRow label={t("workspace.git.stageAll", "同时暂存全部变更")} checked={stageAll} onCheckedChange={setStageAll} />
          <ToggleRow label={t("workspace.git.pushAfter", "提交后推送")} checked={pushAfter} onCheckedChange={setPushAfter} />
          <DialogFooter><Button variant="outline" onClick={() => setCommitOpen(false)}>{t("common.cancel", "取消")}</Button><Button onClick={submitCommit} disabled={!commitSubject.trim() || busy}>{t("workspace.git.commit", "提交")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={prOpen} onOpenChange={setPrOpen}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("workspace.git.createPr", "创建拉取请求")}</DialogTitle><DialogDescription>{t("workspace.git.prHint", "将先推送当前分支；未提交的本地内容不会进入拉取请求。")}</DialogDescription></DialogHeader>
          <Input value={prTitle} onChange={(event) => setPrTitle(event.target.value)} placeholder={t("workspace.git.prTitle", "标题")} />
          <Input value={prBase} onChange={(event) => setPrBase(event.target.value)} placeholder={t("workspace.git.prBase", "目标分支")} />
          <Textarea value={prBody} onChange={(event) => setPrBody(event.target.value)} placeholder={t("workspace.git.prBody", "说明（可选）")} />
          {dirty ? <div className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-300">{t("workspace.git.prDirtyWarning", "当前未提交内容不会进入拉取请求。")}</div> : null}
          <ToggleRow label={t("workspace.git.pushBeforePr", "先推送当前分支")} checked={prPushFirst} onCheckedChange={setPrPushFirst} />
          <ToggleRow label={t("workspace.git.draftPr", "创建为草稿")} checked={prDraft} onCheckedChange={setPrDraft} />
          <DialogFooter><Button variant="outline" onClick={() => setPrOpen(false)}>{t("common.cancel", "取消")}</Button><Button onClick={submitPullRequest} disabled={!prTitle.trim() || !prBase.trim() || busy}>{t("workspace.git.createPr", "创建拉取请求")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}

function GitRow({ icon: Icon, label, value, trailing, onClick, disabled, loading }: { icon: LucideIcon; label: string; value?: string; trailing?: ReactNode; onClick?: () => void; disabled?: boolean; loading?: boolean }) {
  const content = <><Icon className="h-4 w-4 shrink-0" /><span className="min-w-0 flex-1 truncate text-left text-sm font-medium">{label}</span>{value ? <span className="max-w-[45%] truncate text-xs text-muted-foreground">{value}</span> : null}{loading ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : trailing}</>
  if (!onClick) return <div className="flex h-9 items-center gap-2 rounded-lg px-2.5">{content}</div>
  return <button type="button" className="flex h-9 w-full items-center gap-2 rounded-lg px-2.5 transition-colors hover:bg-secondary disabled:cursor-not-allowed disabled:opacity-45" onClick={onClick} disabled={disabled}>{content}</button>
}

function ToggleRow({ label, checked, onCheckedChange }: { label: string; checked: boolean; onCheckedChange: (value: boolean) => void }) {
  return <label className="flex items-center justify-between gap-3 rounded-md border px-3 py-2 text-sm"><span>{label}</span><Switch checked={checked} onCheckedChange={onCheckedChange} /></label>
}
