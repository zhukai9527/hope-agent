// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { SessionGitControlSnapshot, SessionGitDiffSnapshot } from "@/lib/transport"
import { GitControlCard } from "./GitControlCard"

const call = vi.fn()
vi.mock("@/lib/transport-provider", () => ({
  getTransport: () => ({ call }),
}))

function snapshot(patch: Partial<SessionGitControlSnapshot> = {}): SessionGitControlSnapshot {
  return {
    root: "/repo",
    head: "abc123",
    branch: "main",
    detached: false,
    revision: "rev-1",
    branches: [],
    remotes: [],
    worktrees: [],
    dirty: {
      stagedFiles: 0,
      unstagedFiles: 1,
      untrackedFiles: 0,
      conflictedFiles: 0,
      changedFiles: 1,
    },
    status: {
      changedFiles: 1,
      stagedFiles: 0,
      unstagedFiles: 1,
      untrackedFiles: 0,
      conflictedFiles: 0,
      linesAdded: 2,
      linesRemoved: 1,
      clean: false,
    },
    sync: { upstream: null, remote: null, ahead: 0, behind: 0, state: "noUpstream" },
    lastCommit: null,
    activeLocation: "local",
    managedWorktreeId: null,
    capabilities: {
      canSwitchBranch: true,
      canCreateBranch: true,
      canCommit: true,
      canPush: true,
      canCreatePullRequest: false,
      canHandoff: true,
    },
    ...patch,
  }
}

describe("GitControlCard", () => {
  beforeEach(() => call.mockReset())
  afterEach(cleanup)

  it("opens the real unstaged repository diff", async () => {
    const diff: SessionGitDiffSnapshot = {
      revision: "rev-1",
      scope: "unstaged",
      changes: [],
    }
    call.mockResolvedValue(diff)
    const onOpenGitDiff = vi.fn()
    render(
      <GitControlCard
        sessionId="session-1"
        state={{ snapshot: snapshot(), loading: false, error: null, refresh: vi.fn() }}
        managedWorktrees={[]}
        onOpenGitDiff={onOpenGitDiff}
      />,
    )

    fireEvent.click(screen.getByRole("button", { name: /变更|Changes/i }))
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith("load_session_git_diff_snapshot_cmd", {
        sessionId: "session-1",
        scope: "unstaged",
      }),
    )
    expect(onOpenGitDiff).toHaveBeenCalledWith(diff, "session-1")
  })

  it("requires a branch before commit or push in detached worktrees", () => {
    render(
      <GitControlCard
        sessionId="session-1"
        state={{
          snapshot: snapshot({ branch: null, detached: true, activeLocation: "worktree" }),
          loading: false,
          error: null,
          refresh: vi.fn(),
        }}
        managedWorktrees={[]}
        onOpenGitDiff={vi.fn()}
      />,
    )

    expect(
      (screen.getByRole("button", { name: /创建分支|Create branch/i }) as HTMLButtonElement)
        .disabled,
    ).toBe(false)
    expect(
      (screen.getByRole("button", { name: /提交|Commit/i }) as HTMLButtonElement).disabled,
    ).toBe(true)
  })
})
