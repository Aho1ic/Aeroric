import type { Project, Task } from "../types";

export interface TaskSection {
  project: Project;
  tasks: Task[];
}

/** 将桌面回传或推送的任务快照合并到已加载的项目分组。 */
export function upsertTaskInSections(sections: TaskSection[], task: Task): TaskSection[] {
  return sections.map((section) => {
    if (section.project.id !== task.projectId) return section;
    const existingIndex = section.tasks.findIndex((item) => item.id === task.id);
    const tasks = [...section.tasks];
    if (existingIndex < 0) tasks.push(task);
    else tasks[existingIndex] = { ...tasks[existingIndex], ...task };
    tasks.sort((a, b) => b.createdAt - a.createdAt);
    return { ...section, tasks };
  });
}
