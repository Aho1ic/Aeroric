import type { RefObject } from "react";

import type {
  Project,
  ProjectLocation,
  SshConnection,
  TerminalFontSize,
  ThemeVariant,
} from "../../types";
import { ErrorBoundary } from "../ErrorBoundary";
import {
  ShellTerminalPanel,
  type ShellSession,
  type ShellTerminalPanelHandle,
} from "../ShellTerminalPanel";
import { SshTerminalPanel, type SshTerminalPanelHandle } from "../ssh/SshTerminalPanel";
import { WslTerminalPanel, type WslTerminalPanelHandle } from "../wsl/WslTerminalPanel";
import {
  remoteTerminalLayerStyle,
  shellCenterContentStyle,
  shellCenterLayerStyle,
} from "./viewMode";

interface ProjectTerminalsProps {
  project: Project;
  projectLocation: ProjectLocation;
  visible: boolean;
  primaryWorkspaceVisible: boolean;
  terminalDisabled: boolean;
  shellTerminalMounted: boolean;
  shellVisibleInCenter: boolean;
  showShellTerminal: boolean;
  remoteSshMainVisible: boolean;
  showRemoteSshTerminal: boolean;
  remoteConnection?: SshConnection;
  sshConnections: SshConnection[];
  themeVariant: ThemeVariant;
  terminalFontSize: TerminalFontSize;
  shellTerminalFontSize: TerminalFontSize;
  monoFontFamily: string;
  shellLabel: string;
  shellRef: RefObject<ShellTerminalPanelHandle | null>;
  remoteSshRef: RefObject<SshTerminalPanelHandle | null>;
  wslTerminalRef: RefObject<WslTerminalPanelHandle | null>;
  onShellMinimize: () => void;
  onShellClose: () => void;
  onShellReady: () => void;
  onShellSessionsChange: (sessions: ShellSession[], activeShellId: string | null) => void;
  onRemoteSshReady: () => void;
  onWslReady: () => void;
  onSshConnectionsChange: (connections: SshConnection[]) => void;
  onDeleteSshConnection?: (connectionId: string) => void | Promise<void>;
}

/** 项目中心区域的本地、SSH 与 WSL 终端层。三个挂载条件必须保持互斥。 */
export function ProjectTerminals({
  project,
  projectLocation,
  visible,
  primaryWorkspaceVisible,
  terminalDisabled,
  shellTerminalMounted,
  shellVisibleInCenter,
  showShellTerminal,
  remoteSshMainVisible,
  showRemoteSshTerminal,
  remoteConnection,
  sshConnections,
  themeVariant,
  terminalFontSize,
  shellTerminalFontSize,
  monoFontFamily,
  shellLabel,
  shellRef,
  remoteSshRef,
  wslTerminalRef,
  onShellMinimize,
  onShellClose,
  onShellReady,
  onShellSessionsChange,
  onRemoteSshReady,
  onWslReady,
  onSshConnectionsChange,
  onDeleteSshConnection,
}: ProjectTerminalsProps) {
  return (
    <>
      {shellTerminalMounted && projectLocation.kind !== "ssh" && !terminalDisabled && (
        <div style={shellCenterLayerStyle(shellVisibleInCenter)}>
          <div style={shellCenterContentStyle()}>
            <ErrorBoundary label="终端">
              <ShellTerminalPanel
                ref={shellRef}
                projectPath={project.path}
                projectId={project.id}
                isActive={
                  visible && primaryWorkspaceVisible && shellVisibleInCenter && showShellTerminal
                }
                visible={shellVisibleInCenter && showShellTerminal}
                onMinimize={onShellMinimize}
                onClose={onShellClose}
                themeVariant={themeVariant}
                terminalFontSize={shellTerminalFontSize}
                monoFontFamily={monoFontFamily}
                onReady={onShellReady}
                showSessionTabs={false}
                onSessionsChange={onShellSessionsChange}
                shellLabel={shellLabel}
                height="100%"
              />
            </ErrorBoundary>
          </div>
        </div>
      )}

      {showRemoteSshTerminal && remoteConnection && (
        <div style={remoteTerminalLayerStyle(remoteSshMainVisible)}>
          <ErrorBoundary label="SSH">
            <SshTerminalPanel
              ref={remoteSshRef}
              connections={sshConnections}
              onConnectionsChange={onSshConnectionsChange}
              onDeleteConnection={onDeleteSshConnection}
              active={visible && primaryWorkspaceVisible && remoteSshMainVisible}
              width="100%"
              themeVariant={themeVariant}
              terminalFontSize={terminalFontSize}
              monoFontFamily={monoFontFamily}
              initialConnectionId={remoteConnection.id}
              autoConnect
              hideConnectionList
              onReady={onRemoteSshReady}
            />
          </ErrorBoundary>
        </div>
      )}

      {projectLocation.kind === "wsl" && (
        <div style={remoteTerminalLayerStyle(remoteSshMainVisible)}>
          <ErrorBoundary label="WSL">
            <WslTerminalPanel
              ref={wslTerminalRef}
              projectId={project.id}
              distribution={projectLocation.distribution}
              linuxProjectPath={projectLocation.linuxPath}
              active={visible && primaryWorkspaceVisible && remoteSshMainVisible}
              themeVariant={themeVariant}
              terminalFontSize={terminalFontSize}
              monoFontFamily={monoFontFamily}
              onReady={onWslReady}
            />
          </ErrorBoundary>
        </div>
      )}
    </>
  );
}
