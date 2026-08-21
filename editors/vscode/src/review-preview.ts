import * as vscode from 'vscode';
import type { LanguageClient } from 'vscode-languageclient/node';

import {
  renderReviewErrorHtml,
  renderReviewHtml,
  renderReviewLoadingHtml,
} from './review-html';
import { parseReviewDocumentResponse, reviewRequestMethod } from './review-protocol';

export const reviewCommand = 'yanshu.openRustReview';

const reviewViewType = 'yanshu.review';
const refreshDelayMilliseconds = 250;
const maximumReviewPanels = 32;

interface ReviewPanelState {
  readonly sourceUri: vscode.Uri;
  readonly panel: vscode.WebviewPanel;
  requestGeneration: number;
  refreshTimer?: NodeJS.Timeout;
  disposed: boolean;
}

export function registerReviewPreview(
  context: vscode.ExtensionContext,
  client: LanguageClient,
): void {
  const controller = new ReviewPreviewController(client);
  context.subscriptions.push(
    controller,
    vscode.commands.registerCommand(reviewCommand, async () => {
      const source = vscode.window.activeTextEditor?.document;
      if (
        !source
        || source.languageId !== 'yanshu'
        || (source.uri.scheme !== 'file' && source.uri.scheme !== 'untitled')
      ) {
        await vscode.window.showErrorMessage(
          'Open a .yan editor before requesting the read-only Rust-style review.',
        );
        return;
      }
      try {
        await controller.openBeside(source);
      } catch {
        await vscode.window.showErrorMessage(
          'Yanshu could not generate the review preview. Fix .yan diagnostics and try again.',
        );
      }
    }),
  );
}

class ReviewPreviewController implements vscode.Disposable {
  private readonly panels = new Map<string, ReviewPanelState>();
  private readonly subscriptions: vscode.Disposable[];

  constructor(private readonly client: LanguageClient) {
    this.subscriptions = [
      vscode.workspace.onDidChangeTextDocument((event) => this.scheduleRefresh(event.document)),
    ];
  }

  async openBeside(source: vscode.TextDocument): Promise<void> {
    const sourceKey = source.uri.toString();
    const existing = this.panels.get(sourceKey);
    if (existing) {
      existing.panel.reveal(vscode.ViewColumn.Beside, false);
      await this.refresh(existing, source);
      return;
    }
    if (this.panels.size >= maximumReviewPanels) {
      throw new Error('review panel limit reached');
    }
    const sourceLabel = displayName(source.uri);
    const panel = vscode.window.createWebviewPanel(
      reviewViewType,
      `${sourceLabel} · 只读审查`,
      { viewColumn: vscode.ViewColumn.Beside, preserveFocus: false },
      {
        enableScripts: false,
        localResourceRoots: [],
        retainContextWhenHidden: false,
      },
    );
    const state: ReviewPanelState = {
      sourceUri: source.uri,
      panel,
      requestGeneration: 0,
      disposed: false,
    };
    this.panels.set(sourceKey, state);
    panel.onDidDispose(() => {
      state.disposed = true;
      if (state.refreshTimer) {
        clearTimeout(state.refreshTimer);
      }
      this.panels.delete(sourceKey);
    });
    panel.webview.html = renderReviewLoadingHtml(sourceLabel, source.version);
    await this.refresh(state, source);
  }

  dispose(): void {
    for (const subscription of this.subscriptions) {
      subscription.dispose();
    }
    for (const state of [...this.panels.values()]) {
      state.panel.dispose();
    }
    this.panels.clear();
  }

  private scheduleRefresh(document: vscode.TextDocument): void {
    const state = this.panels.get(document.uri.toString());
    if (!state || state.disposed) {
      return;
    }
    if (state.refreshTimer) {
      clearTimeout(state.refreshTimer);
    }
    state.refreshTimer = setTimeout(() => {
      state.refreshTimer = undefined;
      void this.refresh(state, document);
    }, refreshDelayMilliseconds);
  }

  private async refresh(state: ReviewPanelState, source: vscode.TextDocument): Promise<void> {
    const sourceVersion = source.version;
    const generation = state.requestGeneration + 1;
    state.requestGeneration = generation;
    try {
      const response = await this.client.sendRequest<unknown>(reviewRequestMethod, {
        textDocument: {
          uri: source.uri.toString(),
          version: sourceVersion,
        },
      });
      const review = parseReviewDocumentResponse(response, sourceVersion);
      if (
        state.disposed
        || state.requestGeneration !== generation
        || source.version !== sourceVersion
      ) {
        return;
      }
      state.panel.webview.html = renderReviewHtml(review, displayName(state.sourceUri));
    } catch {
      if (state.disposed || state.requestGeneration !== generation) {
        return;
      }
      if (source.version !== sourceVersion) {
        this.scheduleRefresh(source);
        return;
      }
      state.panel.webview.html = renderReviewErrorHtml(
        displayName(state.sourceUri),
        sourceVersion,
      );
    }
  }
}

function displayName(uri: vscode.Uri): string {
  return uri.path.split('/').at(-1) || 'untitled.yan';
}
