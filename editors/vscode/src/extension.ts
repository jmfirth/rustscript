import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

export function activate(context: vscode.ExtensionContext): void {
    const config = vscode.workspace.getConfiguration('rustscript');
    const lspEnabled = config.get<boolean>('lsp.enable', true);

    if (!lspEnabled) {
        return;
    }

    const serverPath = config.get<string>('serverPath', 'rsc');
    const serverArgs = config.get<string[]>('lsp.args', ['lsp']);

    const serverOptions: ServerOptions = {
        command: serverPath,
        args: serverArgs,
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [{ scheme: 'file', language: 'rustscript' }],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.rts'),
        },
        outputChannel: vscode.window.createOutputChannel('RustScript'),
    };

    client = new LanguageClient(
        'rustscript',
        'RustScript Language Server',
        serverOptions,
        clientOptions
    );

    client.start();

    context.subscriptions.push(
        vscode.commands.registerCommand('rustscript.restartServer', async () => {
            if (client) {
                await client.restart();
                vscode.window.showInformationMessage('RustScript language server restarted.');
            }
        })
    );
}

export function deactivate(): Thenable<void> | undefined {
    if (!client) {
        return undefined;
    }
    return client.stop();
}
