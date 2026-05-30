// @vitest-environment jsdom

import { useEffect, useRef, useState } from "react"
import { act, cleanup, render } from "@testing-library/react"
import { afterEach, describe, expect, test, vi } from "vitest"

import type { Message } from "@/types/chat"
import { useNotificationListeners } from "./useNotificationListeners"

const transportMock = vi.hoisted(() => {
  const listeners = new Map<string, (payload: unknown) => void>()
  return {
    listeners,
    listen: vi.fn((eventName: string, handler: (payload: unknown) => void) => {
      listeners.set(eventName, handler)
      return () => {
        listeners.delete(eventName)
      }
    }),
  }
})

vi.mock("@/lib/transport-provider", () => ({
  getTransport: () => transportMock,
}))

vi.mock("@/lib/logger", () => ({
  logger: {
    error: vi.fn(),
  },
}))

vi.mock("@/lib/notifications", () => ({
  notify: vi.fn(),
}))

afterEach(() => {
  cleanup()
  transportMock.listeners.clear()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
  vi.clearAllMocks()
})

function Harness({ onMessages }: { onMessages: (messages: Message[]) => void }) {
  const [messages, setMessages] = useState<Message[]>([])
  const [loading, setLoading] = useState(false)
  const [, setLoadingSessionIds] = useState<Set<string>>(new Set())
  const currentSessionIdRef = useRef<string | null>("parent-session")
  const loadingSessionsRef = useRef<Set<string>>(new Set())
  const sessionCacheRef = useRef<Map<string, Message[]>>(new Map())

  useNotificationListeners({
    currentSessionIdRef,
    setMessages,
    setLoading,
    loadingSessionsRef,
    setLoadingSessionIds,
    sessionCacheRef,
    reloadSessions: async () => {},
  })

  useEffect(() => {
    onMessages(messages)
  }, [messages, onMessages])

  return <div data-loading={loading ? "true" : "false"} />
}

describe("useNotificationListeners", () => {
  test("renders parent-agent stream deltas through the shared chat stream handler", async () => {
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
      cb(0)
      return 1
    })
    vi.stubGlobal("cancelAnimationFrame", () => {})

    let latest: Message[] = []
    render(<Harness onMessages={(messages) => { latest = messages }} />)

    const emit = transportMock.listeners.get("parent_agent_stream")
    expect(emit).toBeTruthy()

    await act(async () => {
      emit?.({ eventType: "started", parentSessionId: "parent-session" })
    })

    await act(async () => {
      emit?.({
        eventType: "delta",
        parentSessionId: "parent-session",
        delta: JSON.stringify({ type: "text_delta", content: "before tool" }),
      })
    })

    expect(latest[0]?.content).toBe("before tool")
    expect(latest[0]?.contentBlocks).toEqual([
      { type: "text", content: "before tool" },
    ])

    await act(async () => {
      emit?.({
        eventType: "delta",
        parentSessionId: "parent-session",
        delta: JSON.stringify({
          type: "tool_call",
          call_id: "call-1",
          name: "read",
          arguments: "{\"path\":\"README.md\"}",
        }),
      })
      emit?.({
        eventType: "delta",
        parentSessionId: "parent-session",
        delta: JSON.stringify({
          type: "tool_result",
          call_id: "call-1",
          result: "ok",
          duration_ms: 5,
        }),
      })
      emit?.({
        eventType: "delta",
        parentSessionId: "parent-session",
        delta: JSON.stringify({ type: "text_delta", content: " after tool" }),
      })
    })

    expect(latest[0]?.content).toBe("before tool after tool")
    expect(latest[0]?.contentBlocks?.map((block) => block.type)).toEqual([
      "text",
      "tool_call",
      "text",
    ])
    expect(latest[0]?.contentBlocks?.[2]).toEqual({
      type: "text",
      content: " after tool",
    })
  })
})
