import * as vscode from 'vscode';
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
} from 'vscode-languageclient/node';

import { registerReviewPreview } from './review-preview';
import { sanitizedServerEnvironment, selectServerCommand } from './server-command';

let client: LanguageClient | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const configuredPath = vscode.workspace
    .getConfiguration('yanshu')
    .get<string>('server.path', '');

  let selection;
  try {
    selection = selectServerCommand({
      extensionPath: context.extensionPath,
      configuredPath,
    });
  } catch {
    await vscode.window.showErrorMessage(
      'Yanshu language server path is invalid. Configure yanshu.server.path with an absolute executable path.',
    );
    return;
  }

  const executable = {
    command: selection.command,
    args: [],
    options: {
      env: sanitizedServerEnvironment(),
    },
  };
  const serverOptions: ServerOptions = {
    run: executable,
    debug: executable,
  };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: 'file', language: 'yanshu' },
      { scheme: 'untitled', language: 'yanshu' },
    ],
  };
  const nextClient = new LanguageClient(
    'yanshu',
    'Yanshu Language Server',
    serverOptions,
    clientOptions,
  );
  client = nextClient;

  try {
    await nextClient.start();
    registerReviewPreview(context, nextClient);
  } catch {
    client = undefined;
    await nextClient.dispose();
    await vscode.window.showErrorMessage(
      'Yanshu language server could not start. Install yanshu-lsp on PATH or configure yanshu.server.path.',
    );
  }
}

export async function deactivate(): Promise<void> {
  const current = client;
  client = undefined;
  if (current) {
    await current.stop();
  }
}
