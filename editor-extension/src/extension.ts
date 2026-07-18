import * as vscode from "vscode";
import { ThreadsModel, type ThreadItem } from "./threads";
import { connectAttach } from "./transport";

const THREADS_VIEW_ID = "muniment.threads";
const REFRESH_COMMAND = "muniment.refreshThreads";

export function activate(context: vscode.ExtensionContext): void {
  const model = new ThreadsModel((onPairingPending) => connectAttach({
    clientVersion: context.extension.packageJSON.version as string,
    onPairingPending,
  }));
  const provider = new ThreadsTreeDataProvider(model);

  context.subscriptions.push(
    model,
    provider,
    vscode.window.registerTreeDataProvider(THREADS_VIEW_ID, provider),
    vscode.commands.registerCommand(REFRESH_COMMAND, () => model.refresh()),
  );
  void model.refresh();
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
