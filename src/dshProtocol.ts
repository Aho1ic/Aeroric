/**
 * DeepSeek Harness protocol snapshot.
 *
 * This is intentionally a small, generated-style compatibility boundary.  The
 * official Harness remains the runtime authority; Aeroric uses this snapshot
 * to advertise which wire surfaces it knows how to bridge and to fail soft
 * when a newer Harness adds a frame instead of silently mis-rendering it.
 */

export const DSH_PROTOCOL_SNAPSHOT = {
  sourceCommit: "99f6f02fecdb7dff40c3fbc9470f5907c29f74ca",
  packageVersion: "0.1.0-rc.7",
  protocolVersion: 2,
  rpcMethods: [
    "session.list",
    "session.search",
    "session.create",
    "session.history",
    "session.models",
    "session.selectModel",
    "session.rename",
    "session.fork",
    "session.prompt",
    "session.attachment",
    "session.updateQueue",
    "session.cancel",
    "subagent.list",
    "subagent.history",
    "subagent.prompt",
    "subagent.interrupt",
    "host.describe",
    "host.pickDirectory",
    "host.listDirectory",
    "host.createDirectory",
    "host.openPath",
    "workspace.list",
    "workspace.create",
    "workspace.rename",
    "workspace.delete",
    "workspace.insertBefore",
    "workspace.insertSessionBefore",
    "workspace.archiveSession",
    "skill.list",
    "agentPreset.list",
    "agentPreset.select",
    "agentPreset.read",
    "agentPreset.copy",
    "agentPreset.openDocument",
    "agentPreset.remove",
    "goal.create",
    "goal.edit",
    "goal.pause",
    "goal.resume",
    "goal.complete",
    "goal.clear",
    "settings.describe",
    "settings.openDocument",
    "settings.update",
    "settings.replace",
    "settings.mutate",
    "credentials.describe",
    "credentials.set",
    "credentials.unset",
    "llm.providers",
    "llm.models",
    "llm.discoverModels",
  ],
  remoteMethods: [
    "commands.list",
    "commands.execute",
    "goals.create",
    "goals.edit",
    "goals.pause",
    "goals.resume",
    "goals.complete",
    "goals.clear",
    "messageFeedback.list",
    "messageFeedback.put",
    "messageFeedback.delete",
    "pluginInventory.list",
    "dynamicCordisRunner.undefineFromPanel",
    "dynamicCordisRunner.runHostHalf",
    "dynamicCordisRunner.getClientCode",
    "dynamicCordisRunner.resolveRequestRun",
    "dynamicCordisRunner.settleUserRun",
    "dynamicCordisRunner.stopFromPanel",
    "dynamicCordisRunner.syncInspectManifest",
    "dynamicCordisRunner.resolveInspectQuery",
    "dynamicCordisRunner.inventory",
    "dynamicCordisRunner.reportRenderFailure",
    "dynamicCordisRunner.reportClientGuardFailure",
    "dynamicCordisRunner.invoke",
  ],
  remoteEvents: [
    "agent-preset/selected",
    "commands/change",
    "credentials/updated",
    "cordis/request-run",
    "cordis/request-run-resolved",
    "cordis/dynamic-package",
    "cordis/dynamic-retract",
    "cordis/inspect-query",
    "cordis/inspect-query-resolved",
    "llm/adapters-updated",
    "settings/document-updated",
  ],
  muxFrames: [
    "session/event",
    "session/subscribed",
    "approval/requested",
    "approval/resolved",
    "question/requested",
    "question/resolved",
    "session/queue",
    "session/jobs",
    "session/projection",
    "stream/error",
  ],
  hostFrames: [
    "host/session-added",
    "host/session-removed",
    "host/session-status",
    "host/agent-error",
    "host/workspace-changed",
    "host/workspace-removed",
    "host/workspace-order-changed",
    "host/archived-sessions-changed",
    "host/remote-event",
    "stream/error",
  ],
  features: {
    structuredHistory: true,
    reasoning: true,
    markdownAndMath: true,
    imageAttachments: true,
    toolPresentation: true,
    trajectory: true,
    stats: true,
    deliverables: true,
    feedback: true,
    schedule: true,
    workflow: true,
    nestedSubagents: true,
    queueAndSteering: true,
    permissions: true,
    plansGoalsTodos: true,
    pluginsAndPresets: true,
    localWslSshTransport: true,
  },
} as const;

export type DshProtocolSnapshot = typeof DSH_PROTOCOL_SNAPSHOT;
export type DshMuxFrameType = (typeof DSH_PROTOCOL_SNAPSHOT.muxFrames)[number];
export type DshHostFrameType = (typeof DSH_PROTOCOL_SNAPSHOT.hostFrames)[number];

export function isKnownDshMuxFrameType(value: unknown): value is DshMuxFrameType {
  return (
    typeof value === "string" &&
    (DSH_PROTOCOL_SNAPSHOT.muxFrames as readonly string[]).includes(value)
  );
}

export function isKnownDshHostFrameType(value: unknown): value is DshHostFrameType {
  return (
    typeof value === "string" &&
    (DSH_PROTOCOL_SNAPSHOT.hostFrames as readonly string[]).includes(value)
  );
}
