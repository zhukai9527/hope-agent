import { useCallback, useEffect, useRef, useState } from "react"

/**
 * Audio recorder hook backed by MediaRecorder + AnalyserNode.
 *
 * Returns a state machine: `idle → requesting-permission → recording →
 * stopped | error`. `cancel` discards the buffered audio; `stop` resolves
 * with the final Blob so the caller can ship it to STT. `audioLevel`
 * (0-1 RMS) feeds the on-screen waveform / red-dot pulse.
 */
export type RecorderState =
  | "idle"
  | "requesting-permission"
  | "recording"
  | "stopped"
  | "error"

export interface UseAudioRecorderResult {
  state: RecorderState
  durationMs: number
  audioLevel: number
  error: Error | null
  /** Begin a new recording. Throws via `error` state if permission is denied. */
  start: () => Promise<void>
  /** Stop and return the recorded Blob. */
  stop: () => Promise<{ blob: Blob; mimeType: string; durationMs: number }>
  /** Discard the current recording and reset to idle. */
  cancel: () => void
}

/** Preferred MediaRecorder MIME types in order of compatibility. */
function pickMimeType(): string {
  if (typeof window === "undefined" || typeof MediaRecorder === "undefined") return ""
  const candidates = [
    "audio/webm;codecs=opus",
    "audio/webm",
    "audio/ogg;codecs=opus",
    "audio/mp4",
    "audio/mpeg",
  ]
  for (const m of candidates) {
    if (MediaRecorder.isTypeSupported?.(m)) return m
  }
  return ""
}

export function useAudioRecorder(): UseAudioRecorderResult {
  const [state, setState] = useState<RecorderState>("idle")
  const [durationMs, setDurationMs] = useState(0)
  const [audioLevel, setAudioLevel] = useState(0)
  const [error, setError] = useState<Error | null>(null)

  const recorderRef = useRef<MediaRecorder | null>(null)
  const chunksRef = useRef<Blob[]>([])
  const streamRef = useRef<MediaStream | null>(null)
  const audioCtxRef = useRef<AudioContext | null>(null)
  const analyserRef = useRef<AnalyserNode | null>(null)
  const rafRef = useRef<number | null>(null)
  const startedAtRef = useRef<number>(0)
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const stopPromiseRef = useRef<{
    resolve: (v: { blob: Blob; mimeType: string; durationMs: number }) => void
    reject: (e: Error) => void
  } | null>(null)

  const cleanup = useCallback(() => {
    if (rafRef.current !== null) cancelAnimationFrame(rafRef.current)
    rafRef.current = null
    if (intervalRef.current !== null) clearInterval(intervalRef.current)
    intervalRef.current = null
    try {
      analyserRef.current?.disconnect()
    } catch {
      // ignore
    }
    analyserRef.current = null
    if (audioCtxRef.current && audioCtxRef.current.state !== "closed") {
      void audioCtxRef.current.close().catch(() => {})
    }
    audioCtxRef.current = null
    streamRef.current?.getTracks().forEach((t) => t.stop())
    streamRef.current = null
    recorderRef.current = null
    setAudioLevel(0)
  }, [])

  const startLevelLoop = useCallback(() => {
    const loop = () => {
      const analyser = analyserRef.current
      if (!analyser) return
      const buf = new Uint8Array(analyser.frequencyBinCount)
      analyser.getByteTimeDomainData(buf)
      let sum = 0
      for (let i = 0; i < buf.length; i++) {
        const v = (buf[i] - 128) / 128
        sum += v * v
      }
      const rms = Math.sqrt(sum / buf.length)
      setAudioLevel(Math.min(1, rms * 2.5))
      rafRef.current = requestAnimationFrame(loop)
    }
    loop()
  }, [])

  const start = useCallback(async () => {
    if (state === "recording" || state === "requesting-permission") return
    setError(null)
    setState("requesting-permission")
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      streamRef.current = stream

      const mimeType = pickMimeType()
      const recorder = mimeType
        ? new MediaRecorder(stream, { mimeType })
        : new MediaRecorder(stream)
      recorderRef.current = recorder
      chunksRef.current = []

      recorder.ondataavailable = (e) => {
        if (e.data && e.data.size > 0) chunksRef.current.push(e.data)
      }
      recorder.onstop = () => {
        const finalMime =
          recorder.mimeType || mimeType || (chunksRef.current[0]?.type ?? "audio/webm")
        const blob = new Blob(chunksRef.current, { type: finalMime })
        const finalDuration = Date.now() - startedAtRef.current
        chunksRef.current = []
        cleanup()
        setState("stopped")
        stopPromiseRef.current?.resolve({
          blob,
          mimeType: finalMime,
          durationMs: finalDuration,
        })
        stopPromiseRef.current = null
      }
      recorder.onerror = (e) => {
        const err = (e as ErrorEvent).error ?? new Error("MediaRecorder error")
        setError(err instanceof Error ? err : new Error(String(err)))
        cleanup()
        setState("error")
        stopPromiseRef.current?.reject(err instanceof Error ? err : new Error(String(err)))
        stopPromiseRef.current = null
      }

      const AudioCtxCtor: typeof AudioContext | undefined =
        (window as unknown as { AudioContext?: typeof AudioContext }).AudioContext ??
        (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
      if (AudioCtxCtor) {
        const ctx = new AudioCtxCtor()
        audioCtxRef.current = ctx
        const source = ctx.createMediaStreamSource(stream)
        const analyser = ctx.createAnalyser()
        analyser.fftSize = 1024
        source.connect(analyser)
        analyserRef.current = analyser
        startLevelLoop()
      }

      recorder.start(1000)
      startedAtRef.current = Date.now()
      setDurationMs(0)
      intervalRef.current = setInterval(() => {
        setDurationMs(Date.now() - startedAtRef.current)
      }, 100)
      setState("recording")
    } catch (e) {
      cleanup()
      setError(e instanceof Error ? e : new Error(String(e)))
      setState("error")
    }
  }, [state, cleanup, startLevelLoop])

  const stop = useCallback(() => {
    return new Promise<{ blob: Blob; mimeType: string; durationMs: number }>(
      (resolve, reject) => {
        const recorder = recorderRef.current
        if (!recorder || recorder.state === "inactive") {
          reject(new Error("Not recording"))
          return
        }
        stopPromiseRef.current = { resolve, reject }
        recorder.stop()
      },
    )
  }, [])

  const cancel = useCallback(() => {
    const recorder = recorderRef.current
    if (recorder && recorder.state !== "inactive") {
      // Suppress onstop resolution by clearing the promise before stop fires.
      stopPromiseRef.current = null
      recorder.onstop = () => {
        cleanup()
        setState("idle")
      }
      recorder.stop()
    } else {
      cleanup()
      setState("idle")
    }
  }, [cleanup])

  // Tear down on unmount.
  useEffect(() => {
    return () => {
      stopPromiseRef.current = null
      cleanup()
    }
  }, [cleanup])

  return { state, durationMs, audioLevel, error, start, stop, cancel }
}
