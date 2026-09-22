import * as vscode from 'vscode';
import { VespertideWebviewProvider } from './webview-provider';
import { AIAgentPanel } from './ai-panel';

export function activate(context: vscode.ExtensionContext): void {
  const provider = new VespertideWebviewProvider(context.extensionUri, context);

  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(
      VespertideWebviewProvider.viewType,
      provider,
      { webviewOptions: { retainContextWhenHidden: true } },
    ),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('vespertide.openAIAgent', () => {
      AIAgentPanel.createOrShow(context.extensionUri, context);
    }),
  );
}

export function deactivate(): void {}
