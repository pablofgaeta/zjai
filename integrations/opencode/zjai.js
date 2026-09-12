// opencode -> zjai bridge.
//
// Status is recorded on disk by zjai-notify, which the zellij
// plugins poll. Delegating to that script keeps the record format and its
// location defined in exactly one place, and costs no zellij IPC: the old
// `zellij pipe` call blocked for a full second per event whenever no plugin
// unblocked the pipe.

const NOTIFY = process.env.HOME
  ? `${process.env.HOME}/.local/libexec/zjai-notify`
  : null;

function args(status) {
  if (
    !NOTIFY ||
    !process.env.ZELLIJ_PANE_ID ||
    !process.env.ZELLIJ_SESSION_NAME
  ) {
    return null;
  }
  return [NOTIFY, "opencode", status];
}

function notify(status) {
  const argv = args(status);
  if (!argv) return;
  try {
    Bun.spawn(argv, { stdout: "ignore", stderr: "ignore" });
  } catch {}
}

// Exit needs the synchronous form: a detached child is not guaranteed to run
// before the process goes away.
function notifyOnExit(status) {
  const argv = args(status);
  if (!argv) return;
  try {
    Bun.spawnSync(argv, { stdout: "ignore", stderr: "ignore" });
  } catch {}
}

process.on("exit", () => notifyOnExit("idle"));

export const ZjaiPlugin = async () => ({
  "chat.message": async () => notify("working"),
  "tool.execute.before": async () => notify("working"),
  "tool.execute.after": async () => notify("working"),
  event: async ({ event }) => {
    switch (event && event.type) {
      case "permission.asked":
      case "question.asked":
        notify("blocked");
        break;
      case "permission.replied":
      case "question.replied":
      case "question.rejected":
        notify("working");
        break;
      case "session.idle":
        notify("done");
        break;
      case "session.error":
        notify("error");
        break;
      case "session.created":
      case "session.deleted":
        notify("idle");
        break;
    }
  },
});

export default ZjaiPlugin;
