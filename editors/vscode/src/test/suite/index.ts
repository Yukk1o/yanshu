import * as assert from 'node:assert/strict';
import { stat } from 'node:fs/promises';
import * as path from 'node:path';

import * as vscode from 'vscode';

const waitTimeoutMilliseconds = 15_000;

export async function run(): Promise<void> {
  console.log('YANSHU_E2E_PHASE: runner-started');
  const extensionId = requiredEnvironment('YANSHU_E2E_EXTENSION_ID');
  const serverPath = requiredEnvironment('YANSHU_E2E_SERVER_PATH');
  const extension = vscode.extensions.getExtension(extensionId);
  assert.ok(extension, `extension is not registered: ${extensionId}`);
  assert.ok(
    path.resolve(serverPath).startsWith(`${path.resolve(extension.extensionPath)}${path.sep}`),
    'test server must be bundled under the isolated extension root',
  );
  assert.ok((await withTimeout(stat(serverPath), 'server stat')).isFile(), 'bundled language server is missing');

  console.log('YANSHU_E2E_PHASE: activating-extension');
  await withTimeout(extension.activate(), 'extension activation');
  assert.equal(extension.isActive, true, 'extension did not activate');
  assert.ok(
    (await withTimeout(vscode.languages.getLanguages(), 'language registration')).includes('yanshu'),
    'Yanshu language contribution is unavailable',
  );

  const workspace = vscode.workspace.workspaceFolders?.[0];
  assert.ok(workspace, 'isolated test workspace is unavailable');
  console.log('YANSHU_E2E_PHASE: checking-language-features');
  await verifyLanguageFeatures(
    vscode.Uri.joinPath(workspace.uri, 'tools.yan'),
    vscode.Uri.joinPath(workspace.uri, 'tools.formatted.txt'),
  );
  console.log('YANSHU_E2E_PHASE: checking-diagnostics');
  await verifyDiagnostics(vscode.Uri.joinPath(workspace.uri, 'broken.yan'));
  console.log('YANSHU_E2E_OK');
}

async function verifyLanguageFeatures(uri: vscode.Uri, expectedFormatUri: vscode.Uri): Promise<void> {
  const document = await withTimeout(vscode.workspace.openTextDocument(uri), 'open tools fixture');
  await withTimeout(vscode.window.showTextDocument(document), 'show tools fixture');
  assert.equal(document.languageId, 'yanshu', '.yan file did not select the Yanshu language');
  await waitFor(() => vscode.languages.getDiagnostics(uri).length === 0);
  const sourceBeforeFormatting = document.getText();
  const localDeclarationMarker = '(fn (value) (target value))';

  const callOffset = sourceBeforeFormatting.lastIndexOf('target value');
  assert.notEqual(callOffset, -1, 'definition fixture call is missing');
  const callPosition = document.positionAt(callOffset);

  const hovers = await withTimeout(vscode.commands.executeCommand<vscode.Hover[]>(
    'vscode.executeHoverProvider',
    uri,
    callPosition,
  ), 'hover request');
  assert.ok(hovers && hovers.length > 0, 'hover provider returned no result');
  const hoverText = hovers.flatMap((hover) => hover.contents).map(markedText).join('\n');
  const normalizedHoverText = normalizePlaintextHover(hoverText);
  assert.match(
    normalizedHoverText,
    /node: expression-v1/u,
    `hover omitted the stable expression node: ${JSON.stringify(hoverText)}`,
  );
  assert.match(
    normalizedHoverText,
    /kind: function definition/u,
    `hover did not classify the selected user function: ${JSON.stringify(hoverText)}`,
  );
  assert.match(
    normalizedHoverText,
    /type: fn\(Int\) -> Int/u,
    `hover omitted the selected user function type: ${JSON.stringify(hoverText)}`,
  );

  const completions = await withTimeout(vscode.commands.executeCommand<vscode.CompletionList>(
    'vscode.executeCompletionItemProvider',
    uri,
    document.positionAt(callOffset + 3),
  ), 'completion request');
  assert.ok(completions, 'completion provider returned no result');
  assert.equal(completions.isIncomplete, false, 'bounded completion must be complete');
  const targetCompletion = completions.items.find(
    (item) => completionLabel(item) === 'target',
  );
  assert.ok(targetCompletion, 'completion omitted the visible target function');
  assert.equal(targetCompletion.kind, vscode.CompletionItemKind.Function);
  assert.match(
    targetCompletion.detail ?? '',
    /fn\(Int\) -> Int/u,
    'completion omitted the target function type',
  );
  assertCompletionEdit(
    document,
    targetCompletion,
    'target',
    new vscode.Range(
      document.positionAt(callOffset),
      document.positionAt(callOffset + 'target'.length),
    ),
  );

  const parameterOffset = callOffset + 'target '.length;
  const parameterCompletions = await withTimeout(
    vscode.commands.executeCommand<vscode.CompletionList>(
      'vscode.executeCompletionItemProvider',
      uri,
      document.positionAt(parameterOffset + 2),
    ),
    'parameter completion request',
  );
  const valueCompletion = parameterCompletions?.items.find(
    (item) => completionLabel(item) === 'value',
  );
  assert.ok(valueCompletion, 'completion omitted the visible function parameter');
  assert.equal(valueCompletion.kind, vscode.CompletionItemKind.Variable);
  assert.match(valueCompletion.detail ?? '', /function parameter · lexical scope/u);
  assertCompletionEdit(
    document,
    valueCompletion,
    'value',
    new vscode.Range(
      document.positionAt(parameterOffset),
      document.positionAt(parameterOffset + 'value'.length),
    ),
  );

  const functionFormOffset = sourceBeforeFormatting.lastIndexOf(localDeclarationMarker) + 1;
  assert.ok(functionFormOffset > 0, 'function form fixture is missing');
  const functionFormHovers = await withTimeout(vscode.commands.executeCommand<vscode.Hover[]>(
    'vscode.executeHoverProvider',
    uri,
    document.positionAt(functionFormOffset),
  ), 'keyword hover request');
  assert.ok(functionFormHovers && functionFormHovers.length > 0, 'keyword hover returned no result');
  const functionFormText = functionFormHovers
    .flatMap((hover) => hover.contents)
    .map(markedText)
    .join('\n');
  const normalizedFunctionFormText = normalizePlaintextHover(functionFormText);
  assert.match(normalizedFunctionFormText, /kind: function special form/u);
  assert.match(normalizedFunctionFormText, /syntax: \(fn \(PARAMETER\.\.\.\) BODY\)/u);

  const definitions = await withTimeout(vscode.commands.executeCommand<Array<vscode.Location | vscode.LocationLink>>(
    'vscode.executeDefinitionProvider',
    uri,
    callPosition,
  ), 'definition request');
  assert.ok(definitions && definitions.length > 0, 'definition provider returned no result');
  const [definition] = definitions;
  assert.ok(definition, 'definition provider returned an empty array');
  const targetUri = definitionTargetUri(definition);
  const targetRange = definitionTargetRange(definition);
  assert.equal(targetUri.toString(), uri.toString(), 'definition escaped the open document');
  assert.equal(document.getText(targetRange), 'target', 'definition resolved to the wrong symbol');

  const globalReferences = await withTimeout(vscode.commands.executeCommand<vscode.Location[]>(
    'vscode.executeReferenceProvider',
    uri,
    callPosition,
  ), 'global references request');
  assert.ok(globalReferences && globalReferences.length > 0, 'global reference provider returned no result');
  assert.ok(
    globalReferences.every((reference) => reference.uri.toString() === uri.toString()),
    'global references escaped the open document',
  );
  assert.ok(
    globalReferences.some((reference) => reference.range.start.isEqual(callPosition)),
    'global references omitted the selected call',
  );
  assert.ok(
    globalReferences.every((reference) => document.getText(reference.range) === 'target'),
    'global references mixed unrelated symbols',
  );

  const localReferenceOffset = callOffset + 'target '.length;
  const localDeclarationOffset = sourceBeforeFormatting.lastIndexOf(localDeclarationMarker)
    + '(fn ('.length;
  assert.ok(
    localDeclarationOffset >= '(fn ('.length,
    'local parameter declaration fixture is missing',
  );
  const localDefinitions = await withTimeout(vscode.commands.executeCommand<Array<vscode.Location | vscode.LocationLink>>(
    'vscode.executeDefinitionProvider',
    uri,
    document.positionAt(localReferenceOffset),
  ), 'local definition request');
  assert.ok(localDefinitions && localDefinitions.length > 0, 'local definition returned no result');
  const [localDefinition] = localDefinitions;
  assert.ok(localDefinition, 'local definition returned an empty array');
  const localRange = definitionTargetRange(localDefinition);
  assert.equal(
    definitionTargetUri(localDefinition).toString(),
    uri.toString(),
    'local definition escaped the open document',
  );
  assert.equal(document.getText(localRange), 'value', 'local definition resolved to the wrong binding');
  assert.ok(
    localRange.start.isEqual(document.positionAt(localDeclarationOffset)),
    'local definition ignored the parameter declaration span',
  );

  const localReferencePosition = document.positionAt(localReferenceOffset);
  const localReferences = await withTimeout(vscode.commands.executeCommand<vscode.Location[]>(
    'vscode.executeReferenceProvider',
    uri,
    localReferencePosition,
  ), 'local references request');
  assert.ok(localReferences && localReferences.length > 0, 'local reference provider returned no result');
  assert.ok(
    localReferences.every((reference) => reference.uri.toString() === uri.toString()),
    'local references escaped the open document',
  );
  assert.ok(
    localReferences.some((reference) => reference.range.start.isEqual(localReferencePosition)),
    'local references omitted the selected parameter use',
  );
  assert.ok(
    localReferences.every((reference) => document.getText(reference.range) === 'value'),
    'local references mixed a shadowed or unrelated symbol',
  );

  const renameEdit = await withTimeout(vscode.commands.executeCommand<vscode.WorkspaceEdit>(
    'vscode.executeDocumentRenameProvider',
    uri,
    localReferencePosition,
    'renamedValue',
  ), 'local rename request');
  assert.ok(renameEdit, 'rename provider returned no workspace edit');
  const renameEntries = renameEdit.entries();
  assert.equal(renameEntries.length, 1, 'same-document rename returned extra document edits');
  const [renameEntry] = renameEntries;
  assert.ok(renameEntry, 'rename workspace edit is empty');
  assert.equal(renameEntry[0].toString(), uri.toString(), 'rename escaped the open document');
  assert.equal(renameEntry[1].length, 2, 'rename omitted a local declaration or reference');
  assert.ok(
    renameEntry[1].every((edit) => (
      document.getText(edit.range) === 'value' && edit.newText === 'renamedValue'
    )),
    'rename mixed unrelated symbols or replacement text',
  );
  assert.equal(
    applyTextEdits(document, renameEntry[1]),
    sourceBeforeFormatting.replace(
      localDeclarationMarker,
      '(fn (renamedValue) (target renamedValue))',
    ),
    'rename edits did not preserve the selected lexical binding',
  );
  assert.equal(document.getText(), sourceBeforeFormatting, 'rename provider changed document text');
  assert.equal(document.isDirty, false, 'rename provider modified the document instead of returning edits');

  const edits = await withTimeout(vscode.commands.executeCommand<vscode.TextEdit[]>(
    'vscode.executeFormatDocumentProvider',
    uri,
    { tabSize: 2, insertSpaces: true },
  ), 'formatting request');
  assert.ok(
    edits && edits.length > 0,
    `formatter did not return edits: ${JSON.stringify(edits)}`,
  );
  const expectedFormat = new TextDecoder().decode(await withTimeout(
    vscode.workspace.fs.readFile(expectedFormatUri),
    'read canonical format fixture',
  ));
  assert.equal(
    applyTextEdits(document, edits),
    expectedFormat,
    'formatting edits did not produce the canonical fixture',
  );
  assert.equal(document.getText(), sourceBeforeFormatting, 'format provider changed document text');
  assert.equal(document.isDirty, false, 'format provider modified the document instead of returning edits');

  await withTimeout(
    vscode.commands.executeCommand('yanshu.openRustReview'),
    'open Rust-style review preview',
  );
  const reviewTab = await waitForResult(() => {
    const current = vscode.window.tabGroups.activeTabGroup.activeTab;
    return current?.input instanceof vscode.TabInputWebview ? current : undefined;
  });
  const reviewInput = reviewTab.input;
  assert.ok(reviewInput instanceof vscode.TabInputWebview, 'review command did not open a webview');
  assert.ok(
    reviewInput.viewType.endsWith('yanshu.review'),
    `review command opened the wrong view: ${reviewInput.viewType}`,
  );
  assert.match(reviewTab.label, /tools\.yan.*只读审查/u);
  assert.equal(
    vscode.workspace.textDocuments.some((candidate) => candidate.uri.scheme === 'yanshu-review'),
    false,
    'review panel exposed an editable text model',
  );
  assert.equal(document.getText(), sourceBeforeFormatting, 'review preview changed canonical .yan source');
  assert.equal(document.isDirty, false, 'review preview dirtied canonical .yan source');
  await withTimeout(
    vscode.commands.executeCommand('workbench.action.closeActiveEditor'),
    'close review preview',
  );

  await withTimeout(
    vscode.commands.executeCommand('workbench.action.closeActiveEditor'),
    'close tools fixture',
  );
}

function applyTextEdits(document: vscode.TextDocument, edits: readonly vscode.TextEdit[]): string {
  const indexed = edits.map((edit) => {
    const start = document.offsetAt(edit.range.start);
    const end = document.offsetAt(edit.range.end);
    assert.ok(document.positionAt(start).isEqual(edit.range.start), 'format edit start is out of range');
    assert.ok(document.positionAt(end).isEqual(edit.range.end), 'format edit end is out of range');
    assert.ok(start <= end, 'format edit range is reversed');
    return { start, end, newText: edit.newText };
  }).sort((left, right) => left.start - right.start || left.end - right.end);

  let previousEnd = 0;
  for (const edit of indexed) {
    assert.ok(previousEnd <= edit.start, 'format edits overlap');
    previousEnd = edit.end;
  }
  let source = document.getText();
  for (const edit of [...indexed].reverse()) {
    source = `${source.slice(0, edit.start)}${edit.newText}${source.slice(edit.end)}`;
  }
  return source;
}

async function verifyDiagnostics(uri: vscode.Uri): Promise<void> {
  const document = await withTimeout(vscode.workspace.openTextDocument(uri), 'open broken fixture');
  await withTimeout(vscode.window.showTextDocument(document), 'show broken fixture');
  const diagnostics = await waitForResult(() => {
    const current = vscode.languages.getDiagnostics(uri);
    return current.some((diagnostic) => String(diagnostic.code) === 'READ_SYNTAX')
      ? current
      : undefined;
  });
  assert.ok(
    diagnostics.some((diagnostic) => diagnostic.source === 'yanshu'),
    'parser diagnostic did not originate from Yanshu',
  );
  await withTimeout(
    vscode.commands.executeCommand('workbench.action.closeActiveEditor'),
    'close broken fixture',
  );
}

function markedText(value: vscode.MarkdownString | vscode.MarkedString): string {
  if (typeof value === 'string') {
    return value;
  }
  return value.value;
}

function normalizePlaintextHover(value: string): string {
  return value
    .replaceAll('&nbsp;', ' ')
    .replaceAll('\\(', '(')
    .replaceAll('\\)', ')')
    .replaceAll('\\-', '-')
    .replaceAll('\\>', '>')
    .replaceAll('\\.', '.');
}

function definitionTargetUri(definition: vscode.Location | vscode.LocationLink): vscode.Uri {
  return definition instanceof vscode.Location ? definition.uri : definition.targetUri;
}

function definitionTargetRange(definition: vscode.Location | vscode.LocationLink): vscode.Range {
  if (definition instanceof vscode.Location) {
    return definition.range;
  }
  return definition.targetSelectionRange ?? definition.targetRange;
}

function completionLabel(item: vscode.CompletionItem): string {
  return typeof item.label === 'string' ? item.label : item.label.label;
}

function assertCompletionEdit(
  document: vscode.TextDocument,
  item: vscode.CompletionItem,
  expectedText: string,
  expectedRange: vscode.Range,
): void {
  const edit = item.textEdit;
  assert.ok(edit, `completion has no text edit: ${completionLabel(item)}`);
  const range = edit.range;
  assert.ok(range.isEqual(expectedRange), 'completion replaced more than the selected symbol');
  assert.equal(document.getText(range), expectedText, 'completion edit selected the wrong source');
  assert.equal(edit.newText, expectedText, 'completion edit inserted unexpected text');
}

async function waitFor(predicate: () => boolean): Promise<void> {
  await waitForResult(() => (predicate() ? true : undefined));
}

async function waitForResult<T>(operation: () => T | undefined): Promise<T> {
  const deadline = Date.now() + waitTimeoutMilliseconds;
  while (Date.now() < deadline) {
    const result = operation();
    if (result !== undefined) {
      return result;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out after ${waitTimeoutMilliseconds} ms`);
}

async function withTimeout<T>(operation: Thenable<T>, label: string): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      Promise.resolve(operation),
      new Promise<T>((_resolve, reject) => {
        timer = setTimeout(
          () => reject(new Error(`${label} timed out after ${waitTimeoutMilliseconds} ms`)),
          waitTimeoutMilliseconds,
        );
      }),
    ]);
  } finally {
    if (timer) {
      clearTimeout(timer);
    }
  }
}

function requiredEnvironment(name: string): string {
  const value = process.env[name]?.trim();
  assert.ok(value, `required test environment is missing: ${name}`);
  return value;
}
