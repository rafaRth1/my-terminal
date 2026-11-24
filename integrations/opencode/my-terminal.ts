// my-terminal status plugin for OpenCode.
//
// OpenCode has no command hooks, so this plugin turns its session events
// into Claude Code shaped hook payloads and pipes them to `my-terminal-hook`,
// which forwards them to the my-terminal sidebar. Outside my-terminal (no
// MY_TERMINAL_* variables) it does nothing.
//
// Install: copy this file to ~/.config/opencode/plugins/my-terminal.ts

import type { Plugin } from "@opencode-ai/plugin"

type Payload = { hook_event_name: string; session_id: string; message?: string }

export const MyTerminal: Plugin = async ({ directory }) => {
  if (!process.env.MY_TERMINAL_SOCKET || !process.env.MY_TERMINAL_PANE) return {}

  // Sub-agent sessions run inside a parent session: only the parent counts.
  const children = new Set<string>()
  // `cargo install` puts the hook in ~/.cargo/bin, which may not be on PATH.
  const hookPath = Bun.which("my-terminal-hook") ?? `${process.env.HOME}/.cargo/bin/my-terminal-hook`

  const send = async (payload: Payload) => {
    if (!payload.session_id || children.has(payload.session_id)) return
    try {
      const hook = Bun.spawn([hookPath], {
        stdin: new Blob([JSON.stringify({ ...payload, cwd: directory })]),
        stdout: "ignore",
        stderr: "ignore",
      })
      await hook.exited
    } catch {
      // A status update must never break OpenCode.
    }
  }

  return {
    event: async ({ event }) => {
      const props = (event as { properties?: any }).properties ?? {}
      const id: string = props.sessionID ?? props.info?.id ?? ""
      switch (event.type) {
        case "session.created":
          if (props.info?.parentID) {
            children.add(id)
            return
          }
          return send({ hook_event_name: "SessionStart", session_id: id })
        case "session.status": {
          const type = props.status?.type
          if (type === "busy" || type === "retry") {
            return send({ hook_event_name: "PreToolUse", session_id: id })
          }
          if (type === "idle") return send({ hook_event_name: "Stop", session_id: id })
          return
        }
        case "permission.asked":
        case "question.asked":
          return send({ hook_event_name: "Notification", session_id: id, message: "Waiting for you" })
        case "session.idle":
        case "session.error":
          return send({ hook_event_name: "Stop", session_id: id })
        case "session.deleted":
          children.delete(id)
          return send({ hook_event_name: "SessionEnd", session_id: id })
      }
    },
  }
}
