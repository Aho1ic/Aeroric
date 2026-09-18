import { Suspense, lazy, type ReactNode } from "react";
import type {
  Project,
  Task,
  ProjectAvatarOverride,
  SkillHubConfig,
  SshConnection,
  ThemeMode,
  ThemeVariant,
  TerminalFontSize,
  TaskDisplayWindow,
  FontFamily,
  ProtocolFamily,
} from "../../types";
import type { SshProjectInput } from "../ssh/sshProject";
import type { WslProjectInput } from "../wsl/WslProjectDialog";
import { WelcomePage } from "../WelcomePage";
import { ReleasePage } from "../ReleasePage";
import { AppSettingsEventHost } from "../AppSettingsEventHost";
import { DshApprovalDialog } from "../DshApprovalDialog";
import { DshQuestionDialog } from "../DshQuestionDialog";
import s from "../../styles";

const ProjectPage = lazy(() =>
  import("../ProjectPage").then((module) => ({ default: module.ProjectPage })),
);

export type AppWorkspaceRoutesProps = {
  children?: ReactNode;
};

/** 纯布局壳：AppProviders 内的根 div。 */
export function AppWorkspaceShell({ children }: AppWorkspaceRoutesProps) {
  return (
    <div style={{ ...s.root, position: "relative" }}>
      <div
        style={{
          position: "absolute",
          inset: 0,
          overflow: "hidden",
        }}
      >
        {children}
      </div>
    </div>
  );
}

export function MountedProjectPages({
  mountedProjects,
  activeProjectId,
  hubProjectId,
  hubMode,
  railProjects,
  sortedProjects,
  tasks,
  onTaskSessionRecovered,
  projectGroups,
  collapsedProjectGroups,
  projectRailWidth,
}: {
  mountedProjects: Project[];
  activeProjectId: string | null;
  hubProjectId: string | undefined;
  hubMode: boolean;
  railProjects: Project[];
  sortedProjects: Project[];
  tasks: Task[];
  onTaskSessionRecovered: (
    taskId: string,
    sessionId: string,
    sessionPath: string,
    codexLike: boolean,
    family?: ProtocolFamily,
  ) => void;
  projectGroups: string[];
  collapsedProjectGroups: Set<string>;
  projectRailWidth: number;
}) {
  return (
    <Suspense fallback={null}>
      {mountedProjects.map((project) => {
        const isHubActive = hubMode && project.id === hubProjectId;
        const railProjectsFiltered = isHubActive
          ? [project]
          : railProjects.filter((p) => p.id !== hubProjectId);
        const otherProjectsFiltered = isHubActive
          ? []
          : sortedProjects.filter((p) => p.id !== project.id && p.id !== hubProjectId);
        return (
          <ProjectPage
            key={project.id}
            project={project}
            visible={activeProjectId === project.id}
            allProjects={railProjectsFiltered}
            otherProjects={otherProjectsFiltered}
            hubMode={isHubActive}
            tasks={tasks}
            onTaskSessionRecovered={onTaskSessionRecovered}
            projectGroups={projectGroups}
            collapsedProjectGroups={collapsedProjectGroups}
            projectRailWidth={projectRailWidth}
          />
        );
      })}
    </Suspense>
  );
}

export function WelcomeOverlay({
  visible,
  projects,
  allProjects,
  tasks,
  onOpen,
  onOpenSshProject,
  onOpenWslProject,
  onProjectClick,
  onDeleteProject,
  onRenameProject,
  onSetProjectAvatar,
  onToggleProjectHidden,
  projectGroups,
  collapsedProjectGroups,
  onCollapsedProjectGroupsChange,
  onAssignProjectGroup,
  onCreateProjectGroup,
  onRenameProjectGroup,
  onDeleteProjectGroup,
  skillHubConfig,
  onEnterSkillHub,
  sshConnections,
  onSshConnectionsChange,
  onDeleteSshConnection,
  themeMode,
  systemPrefersDark,
  onThemeModeChange,
  onToggleTheme,
  onTerminalFontSizeChange,
  onTaskDisplayWindowChange,
  onAttentionBadgeChange,
  sftpLocalDefaultPath,
  onSftpLocalDefaultPathChange,
  onUiFontFamilyChange,
  onMonoFontFamilyChange,
}: {
  visible: boolean;
  projects: Project[];
  allProjects: Project[];
  tasks: Task[];
  onOpen: () => void;
  onOpenSshProject: (input: SshProjectInput) => void;
  onOpenWslProject: (input: WslProjectInput) => void;
  onProjectClick: (p: Project) => void;
  onDeleteProject: (id: string) => void;
  onRenameProject: (id: string, name: string) => void;
  onSetProjectAvatar: (id: string, avatar: ProjectAvatarOverride | undefined) => void;
  onToggleProjectHidden: (id: string) => void;
  projectGroups: string[];
  collapsedProjectGroups: Set<string>;
  onCollapsedProjectGroupsChange: (groups: Set<string>) => void;
  onAssignProjectGroup: (id: string, group: string | null) => void;
  onCreateProjectGroup: (name: string) => void;
  onRenameProjectGroup: (oldName: string, next: string) => void;
  onDeleteProjectGroup: (name: string) => void;
  skillHubConfig: SkillHubConfig | null;
  onEnterSkillHub: () => void;
  sshConnections: SshConnection[];
  onSshConnectionsChange: (connections: SshConnection[]) => void;
  onDeleteSshConnection: (id: string) => void | Promise<void>;
  themeMode: ThemeMode;
  systemPrefersDark: boolean;
  onThemeModeChange: (mode: ThemeMode) => void;
  onToggleTheme: () => void;
  onTerminalFontSizeChange: (size: TerminalFontSize) => void;
  onTaskDisplayWindowChange: (window: TaskDisplayWindow) => void;
  onAttentionBadgeChange: (enabled: boolean) => void;
  sftpLocalDefaultPath: string;
  onSftpLocalDefaultPathChange: (path: string) => void;
  onUiFontFamilyChange: (family: FontFamily) => void;
  onMonoFontFamilyChange: (family: FontFamily) => void;
}) {
  if (!visible) return null;
  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 5,
      }}
    >
      <WelcomePage
        projects={projects}
        allProjects={allProjects}
        tasks={tasks}
        onOpen={onOpen}
        onOpenSshProject={onOpenSshProject}
        onOpenWslProject={onOpenWslProject}
        onProjectClick={onProjectClick}
        onDeleteProject={onDeleteProject}
        onRenameProject={onRenameProject}
        onSetProjectAvatar={onSetProjectAvatar}
        onToggleProjectHidden={onToggleProjectHidden}
        projectGroups={projectGroups}
        collapsedProjectGroups={collapsedProjectGroups}
        onCollapsedProjectGroupsChange={onCollapsedProjectGroupsChange}
        onAssignProjectGroup={onAssignProjectGroup}
        onCreateProjectGroup={onCreateProjectGroup}
        onRenameProjectGroup={onRenameProjectGroup}
        onDeleteProjectGroup={onDeleteProjectGroup}
        skillHubConfig={skillHubConfig}
        onEnterSkillHub={onEnterSkillHub}
        sshConnections={sshConnections}
        onSshConnectionsChange={onSshConnectionsChange}
        onDeleteSshConnection={onDeleteSshConnection}
        themeMode={themeMode}
        systemPrefersDark={systemPrefersDark}
        onThemeModeChange={onThemeModeChange}
        onToggleTheme={onToggleTheme}
        onTerminalFontSizeChange={onTerminalFontSizeChange}
        onTaskDisplayWindowChange={onTaskDisplayWindowChange}
        onAttentionBadgeChange={onAttentionBadgeChange}
        sftpLocalDefaultPath={sftpLocalDefaultPath}
        onSftpLocalDefaultPathChange={onSftpLocalDefaultPathChange}
        onUiFontFamilyChange={onUiFontFamilyChange}
        onMonoFontFamilyChange={onMonoFontFamilyChange}
      />
    </div>
  );
}

export function AppShellOverlays({
  themeMode,
  themeVariant,
  systemPrefersDark,
  onThemeModeChange,
  terminalFontSize,
  onTerminalFontSizeChange,
  taskDisplayWindow,
  onTaskDisplayWindowChange,
  attentionBadge,
  onAttentionBadgeChange,
  sftpLocalDefaultPath,
  onSftpLocalDefaultPathChange,
  uiFontFamily,
  onUiFontFamilyChange,
  monoFontFamily,
  onMonoFontFamilyChange,
  dshWebSearchEnabled,
  onDshWebSearchEnabledChange,
  showReleasePage,
  onCloseReleasePage,
  dshApprovalRequest,
  onCloseApproval,
  dshQuestionRequest,
  onCloseQuestion,
}: {
  themeMode: ThemeMode;
  themeVariant: ThemeVariant;
  systemPrefersDark: boolean;
  onThemeModeChange: (mode: ThemeMode) => void;
  terminalFontSize: TerminalFontSize;
  onTerminalFontSizeChange: (size: TerminalFontSize) => void;
  taskDisplayWindow: TaskDisplayWindow;
  onTaskDisplayWindowChange: (window: TaskDisplayWindow) => void;
  attentionBadge: boolean;
  onAttentionBadgeChange: (enabled: boolean) => void;
  sftpLocalDefaultPath: string;
  onSftpLocalDefaultPathChange: (path: string) => void;
  uiFontFamily: FontFamily;
  onUiFontFamilyChange: (family: FontFamily) => void;
  monoFontFamily: FontFamily;
  onMonoFontFamilyChange: (family: FontFamily) => void;
  dshWebSearchEnabled: boolean;
  onDshWebSearchEnabledChange: (enabled: boolean) => void;
  showReleasePage: boolean;
  onCloseReleasePage: () => void;
  dshApprovalRequest: unknown;
  onCloseApproval: () => void;
  dshQuestionRequest: unknown;
  onCloseQuestion: () => void;
}) {
  return (
    <>
      <AppSettingsEventHost
        themeMode={themeMode}
        themeVariant={themeVariant}
        systemPrefersDark={systemPrefersDark}
        onThemeModeChange={onThemeModeChange}
        terminalFontSize={terminalFontSize}
        onTerminalFontSizeChange={onTerminalFontSizeChange}
        taskDisplayWindow={taskDisplayWindow}
        onTaskDisplayWindowChange={onTaskDisplayWindowChange}
        attentionBadge={attentionBadge}
        onAttentionBadgeChange={onAttentionBadgeChange}
        sftpLocalDefaultPath={sftpLocalDefaultPath}
        onSftpLocalDefaultPathChange={onSftpLocalDefaultPathChange}
        uiFontFamily={uiFontFamily}
        onUiFontFamilyChange={onUiFontFamilyChange}
        monoFontFamily={monoFontFamily}
        onMonoFontFamilyChange={onMonoFontFamilyChange}
        dshWebSearchEnabled={dshWebSearchEnabled}
        onDshWebSearchEnabledChange={onDshWebSearchEnabledChange}
      />
      {showReleasePage && <ReleasePage onClose={onCloseReleasePage} />}
      <DshApprovalDialog request={dshApprovalRequest as never} onClose={onCloseApproval} />
      <DshQuestionDialog request={dshQuestionRequest as never} onClose={onCloseQuestion} />
    </>
  );
}
