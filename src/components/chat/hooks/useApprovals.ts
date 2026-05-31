import { useState, useEffect, useMemo } from "react"
import { getTransport } from "@/lib/transport-provider"
import { parsePayload } from "@/lib/transport"
import { logger } from "@/lib/logger"
import type { ApprovalRequest } from "@/components/chat/ApprovalDialog"

export interface UseApprovalsReturn {
  approvalRequests: ApprovalRequest[]
  handleApprovalResponse: (
    requestId: string,
    response: "allow_once" | "allow_always" | "deny",
  ) => Promise<void>
}

export function useApprovals(currentSessionId: string | null): UseApprovalsReturn {
  const [allApprovalRequests, setAllApprovalRequests] = useState<ApprovalRequest[]>([])
  const approvalRequests = useMemo(
    () =>
      allApprovalRequests.filter((request) => {
        if (!request.session_id) return true
        return request.session_id === currentSessionId
      }),
    [allApprovalRequests, currentSessionId],
  )

  // Listen for command approval events
  useEffect(() => {
    return getTransport().listen("approval_required", (raw) => {
      try {
        setAllApprovalRequests((prev) => [...prev, parsePayload<ApprovalRequest>(raw)])
      } catch (e) {
        logger.error("ui", "ChatScreen::approval", "Failed to parse approval request", e)
      }
    })
  }, [])

  // Dismiss a card when its approval is settled via ANY path other than this
  // GUI's own click — a desktop-pet hook / IM button answering first
  // (`approval_resolved`), or a backend-enforced timeout (`approval_timed_out`).
  // Without this the card only clears on its own submit, so an externally-
  // answered or expired approval would leave a stale, still-clickable dialog.
  // Both events carry snake_case `request_id`; matched on that globally unique
  // id (a stale id for another session just no-ops the filter).
  useEffect(() => {
    const transport = getTransport()
    const dismiss = (raw: unknown) => {
      try {
        const requestId = parsePayload<{ request_id?: string }>(raw)?.request_id
        if (!requestId) return
        setAllApprovalRequests((prev) => prev.filter((r) => r.request_id !== requestId))
      } catch (e) {
        logger.error("ui", "ChatScreen::approval", "Failed to parse approval resolution", e)
      }
    }
    const offResolved = transport.listen("approval_resolved", dismiss)
    const offTimedOut = transport.listen("approval_timed_out", dismiss)
    return () => {
      offResolved()
      offTimedOut()
    }
  }, [])

  async function handleApprovalResponse(
    requestId: string,
    response: "allow_once" | "allow_always" | "deny",
  ) {
    setAllApprovalRequests((prev) => prev.filter((r) => r.request_id !== requestId))
    try {
      await getTransport().call("respond_to_approval", { requestId, response })
    } catch (e) {
      logger.error("ui", "ChatScreen::approval", "Failed to respond to approval", e)
    }
  }

  return { approvalRequests, handleApprovalResponse }
}
