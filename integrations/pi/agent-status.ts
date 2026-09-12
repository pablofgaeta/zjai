// pi -> agent-status bridge.
//
// Status is recorded on disk by agent-status-notify, which the zellij plugins
// poll. Delegating to that script keeps the record format and its location
// defined in exactly one place, and costs no zellij IPC: the old `zellij pipe`
// call blocked for a full second per event whenever no plugin unblocked the
// pipe.

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { spawn, spawnSync } from "node:child_process";

type AgentStatus =
  "working" | "blocked" | "done" | "idle" | "unknown" | "error";

const NOTIFY = process.env.HOME
  ? `${process.env.HOME}/.local/libexec/agent-status-notify`
  : null;

function args(status: AgentStatus): [string, string[]] | null {
  if (
    !NOTIFY ||
    !process.env.ZELLIJ_PANE_ID ||
    !process.env.ZELLIJ_SESSION_NAME
  ) {
    return null;
  }
  return [NOTIFY, ["pi", status]];
}

function notify(status: AgentStatus) {
  const argv = args(status);
  if (!argv) return;
  try {
    const child = spawn(argv[0], argv[1], { detached: true, stdio: "ignore" });
    child.on("error", () => {});
    child.unref();
  } catch {}
}

// Exit needs the synchronous form: a detached child is not guaranteed to run
// before the process goes away.
function notifyOnExit(status: AgentStatus) {
  const argv = args(status);
  if (!argv) return;
  try {
    spawnSync(argv[0], argv[1], { stdio: "ignore" });
  } catch {}
}

process.on("exit", () => notifyOnExit("idle"));

export default function (pi: ExtensionAPI) {
  let activeAgentRuns = 0;
  let activePrompts = 0;

  function notifyAgentState(status: AgentStatus) {
    if (activePrompts > 0 && status === "working") {
      notify("blocked");
      return;
    }
    notify(status);
  }

  function resumeAfterPrompt(ctx: { isIdle(): boolean }) {
    if (activePrompts > 0) {
      notify("blocked");
    } else if (activeAgentRuns > 0 || !ctx.isIdle()) {
      notify("working");
    } else {
      notify("idle");
    }
  }

  pi.on("session_start", () => {
    notify("idle");
  });

  pi.on("before_agent_start", () => {
    notifyAgentState("working");
  });

  pi.on("agent_start", () => {
    activeAgentRuns += 1;
    notifyAgentState("working");
  });

  pi.on("ui_prompt_start", () => {
    activePrompts += 1;
    notify("blocked");
  });

  pi.on("ui_prompt_end", (_event, ctx) => {
    activePrompts = Math.max(0, activePrompts - 1);
    resumeAfterPrompt(ctx);
  });

  pi.on("tool_call", () => {
    notifyAgentState("working");
  });

  pi.on("tool_execution_start", () => {
    notifyAgentState("working");
  });

  pi.on("tool_execution_update", () => {
    notifyAgentState("working");
  });

  pi.on("tool_execution_end", (event) => {
    // A failed tool call flashes red immediately, but doesn't stick: if the
    // agent recovers and keeps going, later events (another tool call,
    // agent_settled) overwrite it.
    notifyAgentState(event.isError ? "error" : "working");
  });

  pi.on("agent_end", () => {
    activeAgentRuns = Math.max(0, activeAgentRuns - 1);
  });

  pi.on("agent_settled", (_event, ctx) => {
    activeAgentRuns = 0;
    activePrompts = 0;
    notify(ctx.isIdle() ? "done" : "working");
  });

  pi.on("session_shutdown", () => {
    notifyOnExit("idle");
  });
}
