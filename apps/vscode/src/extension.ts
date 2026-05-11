import * as vscode from 'vscode';
import { VespertideWebviewProvider } from './webview-provider';

export function activate(context: vscode.ExtensionContext): void {
  const provider = new VespertideWebviewProvider(context.extensionUri, context);

  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(
      VespertideWebviewProvider.viewType,
      provider,
      { webviewOptions: { retainContextWhenHidden: true } },
    ),
  );
}

export function deactivate(): void {}
