import * as vscode from 'vscode';
import { UvCliClient } from './cliClient';
import { UvisionIdePanel } from './webviewPanel';

let cliClient: UvCliClient | undefined;

export function activate(context: vscode.ExtensionContext): void {
  cliClient = new UvCliClient(context.extensionPath);

  const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 95);
  statusBar.text = '$(circuit-board) µVision Fast';
  statusBar.tooltip = 'Open Keil µVision 5 Replica IDE';
  statusBar.command = 'uvision-fast.open';
  statusBar.show();
  context.subscriptions.push(statusBar);

  context.subscriptions.push(
    vscode.commands.registerCommand('uvision-fast.open', () => {
      UvisionIdePanel.createOrShow(context, cliClient!);
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('uvision-fast.openFile', (uri?: vscode.Uri) => {
      const target =
        uri?.fsPath || vscode.window.activeTextEditor?.document.uri.fsPath;
      UvisionIdePanel.createOrShow(context, cliClient!, target);
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('uvision-fast.build', () => {
      const target = vscode.window.activeTextEditor?.document.uri.fsPath;
      const panel = UvisionIdePanel.createOrShow(context, cliClient!, target);
      panel.sendCommand('build');
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('uvision-fast.debug', () => {
      const target = vscode.window.activeTextEditor?.document.uri.fsPath;
      const panel = UvisionIdePanel.createOrShow(context, cliClient!, target);
      panel.sendCommand('debug');
    })
  );
}

export function deactivate(): void {
  cliClient?.dispose();
  cliClient = undefined;
}
