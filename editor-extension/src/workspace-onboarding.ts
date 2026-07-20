import { isAbsolute } from "node:path";

export interface WorkspaceFolderLike { readonly key: string; readonly scheme: string; readonly fsPath: string; }
export interface WorkspaceRuntime {
  onboardWorkspace(openedDirectory: string, memoryLocation: string): Promise<unknown>;
  dispose(): void;
}

export class WorkspaceOnboarding {
  private readonly active = new Map<string, { memory: string; promise: Promise<void> }>();

  constructor(
    private readonly overrideFor: (folder: WorkspaceFolderLike) => string,
    private readonly connect: () => Promise<WorkspaceRuntime>,
    private readonly report: (message: string) => void,
  ) {}

  initialize(folders: readonly WorkspaceFolderLike[]): Promise<void> {
    return Promise.all(folders.filter((folder) => folder.scheme === "file")
      .map((folder) => this.initializeFolder(folder))).then(() => undefined);
  }

  initializeFolder(folder: WorkspaceFolderLike): Promise<void> {
    if (folder.scheme !== "file") return Promise.resolve();
    const configured = this.overrideFor(folder).trim();
    const memory = configured || folder.fsPath;
    const existing = this.active.get(folder.key);
    if (existing?.memory === memory) return existing.promise;
    let task!: Promise<void>;
    task = (existing?.promise ?? Promise.resolve())
      .then(() => this.run(folder, memory))
      .finally(() => {
        if (this.active.get(folder.key)?.promise === task) this.active.delete(folder.key);
      });
    this.active.set(folder.key, { memory, promise: task });
    return task;
  }

  private async run(folder: WorkspaceFolderLike, memory: string): Promise<void> {
    if (!isAbsolute(memory)) {
      this.report("Muniment workspace memory must be an absolute directory.");
      return;
    }
    let runtime: WorkspaceRuntime | undefined;
    try {
      runtime = await this.connect();
      await runtime.onboardWorkspace(folder.fsPath, memory);
    } catch {
      this.report("Muniment couldn’t initialize workspace memory. Open the desktop app and check the workspace setting.");
    } finally {
      runtime?.dispose();
    }
  }
}
