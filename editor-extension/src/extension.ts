import * as vscode from "vscode";
import { ThreadDocumentLoader } from "./thread-document";
import { openAcceptedRun, permissionDecision, RunDocumentStore } from "./run-documents";
import { NEW_RUN_COMMAND, NEW_RUN_WITH_CURRENT_FILE_COMMAND, OPEN_THREAD_COMMAND, ThreadsModel, threadOpenCommand, type ThreadItem } from "./threads";
import { connectAttach } from "./transport";
import { editorFileContext, editorSelectionContext, type ActiveFile, type ActiveSelection } from "./editor-context";
import { WorkspaceOnboarding, type WorkspaceFolderLike } from "./workspace-onboarding";

const THREADS_VIEW_ID = "muniment.threads";
const REFRESH_COMMAND = "muniment.refreshThreads";
const THREAD_SCHEME = "muniment-thread";
const RUN_SCHEME = "muniment-run";

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const onboarding = new WorkspaceOnboarding(
    (folder) => vscode.workspace.getConfiguration("muniment", vscode.Uri.parse(folder.key))
      .get<string>("workspaceMemoryLocation", ""),
    () => connectAttach({
      clientVersion: context.extension.packageJSON.version as string,
      approvalTimeoutMs: 10_000,
    }),
    (message) => { void vscode.window.showErrorMessage(message); },
  );
  const folders = () => (vscode.workspace.workspaceFolders ?? []).map(workspaceFolderLike);
  const model = new ThreadsModel((onPairingPending) => connectAttach({
    clientVersion: context.extension.packageJSON.version as string,
    onPairingPending,
  }));
  const provider = new ThreadsTreeDataProvider(model);
  const documents = new ThreadDocumentProvider();
  const runDocuments = new RunDocumentProvider();
  const loader = new ThreadDocumentLoader((threadId) => model.openThread(threadId));

  context.subscriptions.push(
    model,
    provider,
    loader,
    documents,
    runDocuments,
    vscode.window.registerTreeDataProvider(THREADS_VIEW_ID, provider),
    vscode.workspace.registerTextDocumentContentProvider(THREAD_SCHEME, documents),
    vscode.workspace.registerTextDocumentContentProvider(RUN_SCHEME, runDocuments),
    vscode.commands.registerCommand(REFRESH_COMMAND, () => {
      runDocuments.clear();
      return model.refresh();
    }),
    vscode.commands.registerCommand(NEW_RUN_COMMAND, () => submitRun(activeEditorSelectionContext())),
    vscode.commands.registerCommand(NEW_RUN_WITH_CURRENT_FILE_COMMAND, async () => {
      const runContext = activeEditorFileContext();
      if (runContext === undefined) {
        await vscode.window.showWarningMessage("Open a file in the current workspace and try again.");
        return;
      }
      await submitRun(runContext);
    }),
    vscode.commands.registerCommand(OPEN_THREAD_COMMAND, async (threadId: string, title: string) => {
      if (typeof threadId !== "string" || typeof title !== "string") return;
      const result = await vscode.window.withProgress({
        location: vscode.ProgressLocation.Notification,
        title: `Opening ${title}…`,
      }, () => loader.load(threadId, title));
      if (result.kind === "stale") return;
      if (result.kind === "error") {
        void vscode.window.showErrorMessage(result.message);
        return;
      }
      const uri = threadUri(threadId);
      documents.set(uri, result.content);
      const document = await vscode.workspace.openTextDocument(uri);
      await vscode.window.showTextDocument(document, { preview: true });
    }),
    vscode.workspace.onDidChangeWorkspaceFolders((event) => {
      void onboarding.initialize(event.added.map(workspaceFolderLike));
    }),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("muniment.workspaceMemoryLocation")) {
        void onboarding.initialize(folders());
      }
    }),
  );
  void onboarding.initialize(folders()).finally(() => model.refresh());

  async function submitRun(runContext?: ReturnType<typeof editorSelectionContext>): Promise<void> {
    const text = await vscode.window.showInputBox({
      prompt: "Start a new Muniment run",
      placeHolder: "What would you like Muniment to do?",
    });
    if (text === undefined || text.trim().length === 0) return;
    const result = await vscode.window.withProgress({
      location: vscode.ProgressLocation.Notification,
      title: "Starting Muniment run…",
    }, () => model.submitRun(text, runContext));
    if (result.kind === "accepted") {
      await openAcceptedRun(result, runDocuments.store, {
        streamRun: (runId, afterRunSeq) => model.streamRun(runId, afterRunSeq),
        promptPermission: async ({ title, message }) => {
          const action = await vscode.window.showWarningMessage(
            `Permission required: ${title}`,
            { modal: true, ...(message === undefined ? {} : { detail: message }) },
            "Allow",
            "Deny",
          );
          return permissionDecision(action);
        },
        answerPermission: (runId, gateId, decision) =>
          model.answerPermission(runId, gateId, decision),
        showDocument: async (uri) => {
          const document = await vscode.workspace.openTextDocument(vscode.Uri.parse(uri));
          await vscode.window.showTextDocument(document, { preview: true });
        },
      });
    } else if (result.kind === "busy") {
      void vscode.window.showWarningMessage("A Muniment run is already being submitted.");
    } else if (result.kind === "unavailable") {
      void vscode.window.showErrorMessage("Muniment isn’t connected. Refresh Threads and try again.");
    } else if (result.kind === "failed") {
      void vscode.window.showErrorMessage(result.message);
    }
  }
}

function workspaceFolderLike(folder: vscode.WorkspaceFolder): WorkspaceFolderLike {
  return { key: folder.uri.toString(), scheme: folder.uri.scheme, fsPath: folder.uri.fsPath };
}

function activeEditorFileContext() {
  const document = vscode.window.activeTextEditor?.document;
  if (!document) return undefined;
  const workspaceFolder = document.uri.scheme === "file"
    ? vscode.workspace.getWorkspaceFolder(document.uri)
    : undefined;
  const snapshot: ActiveFile = {
    scheme: document.uri.scheme,
    text: workspaceFolder ? document.getText() : "",
    workspaceRelativePath: workspaceFolder
      ? vscode.workspace.asRelativePath(document.uri, false)
      : undefined,
  };
  return editorFileContext(snapshot);
}

function activeEditorSelectionContext() {
  const editor = vscode.window.activeTextEditor;
  if (!editor) return undefined;
  const { document, selection } = editor;
  const workspaceFolder = document.uri.scheme === "file" && !selection.isEmpty
    ? vscode.workspace.getWorkspaceFolder(document.uri)
    : undefined;
  const snapshot: ActiveSelection = {
    scheme: document.uri.scheme,
    isEmpty: selection.isEmpty,
    selectedText: workspaceFolder ? document.getText(selection) : "",
    workspaceRelativePath: workspaceFolder
      ? vscode.workspace.asRelativePath(document.uri, false)
      : undefined,
  };
  return editorSelectionContext(snapshot);
}

export function deactivate(): void {
  // Resources registered with the extension context are disposed by VS Code.
}

class ThreadsTreeDataProvider implements vscode.TreeDataProvider<ThreadItem>, vscode.Disposable {
  private readonly emitter = new vscode.EventEmitter<ThreadItem | undefined | null | void>();
  private readonly modelSubscription: { dispose(): void };
  readonly onDidChangeTreeData = this.emitter.event;

  constructor(private readonly model: ThreadsModel) {
    this.modelSubscription = model.onDidChange((state) => {
      void vscode.commands.executeCommand("setContext", "muniment.threadsState", state.kind);
      this.emitter.fire();
    });
    void vscode.commands.executeCommand("setContext", "muniment.threadsState", model.state.kind);
  }

  getTreeItem(item: ThreadItem): vscode.TreeItem {
    const treeItem = new vscode.TreeItem(item.title, vscode.TreeItemCollapsibleState.None);
    treeItem.id = item.threadId;
    treeItem.description = item.description;
    treeItem.iconPath = new vscode.ThemeIcon("comment-discussion");
    treeItem.command = threadOpenCommand(item);
    return treeItem;
  }

  getChildren(element?: ThreadItem): ThreadItem[] {
    if (element || this.model.state.kind !== "ready") return [];
    return this.model.state.threads;
  }

  dispose(): void {
    this.modelSubscription.dispose();
    this.emitter.dispose();
  }
}

function threadUri(threadId: string): vscode.Uri {
  return vscode.Uri.from({ scheme: THREAD_SCHEME, path: `/${encodeURIComponent(threadId)}.md` });
}

class ThreadDocumentProvider implements vscode.TextDocumentContentProvider, vscode.Disposable {
  private readonly emitter = new vscode.EventEmitter<vscode.Uri>();
  private readonly contents = new Map<string, string>();
  readonly onDidChange = this.emitter.event;

  provideTextDocumentContent(uri: vscode.Uri): string {
    return this.contents.get(uri.toString()) ?? "";
  }

  set(uri: vscode.Uri, content: string): void {
    this.contents.set(uri.toString(), content);
    this.emitter.fire(uri);
  }

  dispose(): void {
    this.contents.clear();
    this.emitter.dispose();
  }
}

class RunDocumentProvider implements vscode.TextDocumentContentProvider, vscode.Disposable {
  private readonly emitter = new vscode.EventEmitter<vscode.Uri>();
  readonly store = new RunDocumentStore((uri) => this.emitter.fire(vscode.Uri.parse(uri)));
  readonly onDidChange = this.emitter.event;

  provideTextDocumentContent(uri: vscode.Uri): string {
    return this.store.content(uri.toString());
  }

  dispose(): void {
    this.store.clear();
    this.emitter.dispose();
  }

  clear(): void {
    this.store.clear();
  }
}
