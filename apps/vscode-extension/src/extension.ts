import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
  RevealOutputChannelOn,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let statusBarItem: vscode.StatusBarItem;

function getPlatformDir(): string {
  const arch = os.arch();
  const plat = os.platform();
  const key = `${plat}-${arch}`;
  const map: Record<string, string> = {
    "linux-x64": "linux-x64",
    "linux-arm64": "linux-arm64",
    "darwin-x64": "darwin-x64",
    "darwin-arm64": "darwin-arm64",
    "win32-x64": "win32-x64",
  };
  const dir = map[key];
  if (!dir) throw new Error(`Unsupported platform: ${key}`);
  return dir;
}

function findOnPath(exe: string): string | undefined {
  const env = process.env.PATH ?? "";
  const sep = os.platform() === "win32" ? ";" : ":";
  for (const dir of env.split(sep)) {
    if (!dir) continue;
    const candidate = path.join(dir, exe);
    if (fs.existsSync(candidate)) return candidate;
  }
  return undefined;
}

function resolveServerBinary(context: vscode.ExtensionContext): string {
  const config = vscode.workspace.getConfiguration("vespertide");
  const override = config.get<string>("serverPath");
  if (override && override.trim() !== "") {
    if (!fs.existsSync(override)) {
      throw new Error(`vespertide.serverPath points to a non-existent file: ${override}`);
    }
    return override;
  }

  const exe = os.platform() === "win32" ? "vespertide-lsp.exe" : "vespertide-lsp";
  const bundled = context.asAbsolutePath(path.join("bin", getPlatformDir(), exe));
  if (fs.existsSync(bundled)) {
    return bundled;
  }

  // Dev convenience: when the bundled binary is missing (`cargo install` /
  // local debug builds), fall back to whatever `vespertide-lsp` exists on
  // PATH. This is the same UX Zed offers and removes the need to set
  // `vespertide.serverPath` while iterating on the LSP.
  const onPath = findOnPath(exe);
  if (onPath) {
    return onPath;
  }

  throw new Error(
    `Vespertide LSP binary not found.\n` +
      `Looked for bundled: ${bundled}\n` +
      `Looked on PATH for: ${exe}\n` +
      `Set "vespertide.serverPath", install via \`cargo install vespertide-cli\`, or reinstall the extension.`
  );
}

function createStatusBarItem(): vscode.StatusBarItem {
  const item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  item.text = "$(loading~spin) Vespertide";
  item.tooltip = "Vespertide Language Server";
  item.command = "vespertide.restartServer";
  item.show();
  return item;
}

async function startClient(context: vscode.ExtensionContext): Promise<void> {
  let serverPath: string;
  try {
    serverPath = resolveServerBinary(context);
  } catch (err) {
    statusBarItem.text = "$(error) Vespertide: Not Found";
    void vscode.window.showErrorMessage(`Vespertide: ${(err as Error).message}`);
    return;
  }

  // Surface the binary path so a stale F5 / cached dev-host can be
  // diagnosed at a glance. The full path lives in the status bar tooltip
  // and is logged so it ends up in both VS Code's output channel and the
  // LSP's own file log.
  console.log(`[vespertide] launching LSP server from: ${serverPath}`);

  const config = vscode.workspace.getConfiguration("vespertide");
  const logLevel = config.get<string>("logLevel", "info");

  const serverOptions: ServerOptions = {
    run: {
      command: serverPath,
      args: [],
      transport: TransportKind.stdio,
      options: { env: { ...process.env, RUST_LOG: `vespertide_lsp=${logLevel}` } },
    },
    debug: {
      command: serverPath,
      args: [],
      transport: TransportKind.stdio,
      options: { env: { ...process.env, RUST_LOG: "vespertide_lsp=trace" } },
    },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "vespertide-json" },
      { scheme: "file", language: "vespertide-yaml" },
    ],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher(
        "**/{models,migrations}/*.{json,yaml,yml}"
      ),
    },
    revealOutputChannelOn: RevealOutputChannelOn.Error,
    traceOutputChannel: vscode.window.createOutputChannel("Vespertide LSP Trace"),
  };

  client = new LanguageClient("vespertide", "Vespertide", serverOptions, clientOptions);

  try {
    await client.start();
    statusBarItem.text = "$(check) Vespertide";
    statusBarItem.tooltip = `Vespertide LSP (connected)\nBinary: ${serverPath}`;
  } catch (err) {
    statusBarItem.text = "$(error) Vespertide";
    void vscode.window.showErrorMessage(`Vespertide LSP failed to start: ${err}`);
  }
}

async function stopClient(): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  statusBarItem = createStatusBarItem();
  context.subscriptions.push(statusBarItem);

  context.subscriptions.push(
    vscode.commands.registerCommand("vespertide.restartServer", async () => {
      statusBarItem.text = "$(loading~spin) Vespertide: Restarting";
      await stopClient();
      await startClient(context);
    })
  );

  await startClient(context);
}

export async function deactivate(): Promise<void> {
  await stopClient();
}
