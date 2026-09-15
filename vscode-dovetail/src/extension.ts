import * as net from "net";
import * as vscode from "vscode";
import { workspace, window, commands, ExtensionContext } from "vscode";
import {
  CloseAction,
  ErrorAction,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  StreamInfo,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

function runDovetailTask(name: string, args: string) {
  const config = workspace.getConfiguration("dovetail");
  const serverPath = config.get<string>("serverPath", "dovetail");
  const task = new vscode.Task(
    { type: "dovetail", task: name },
    vscode.TaskScope.Workspace,
    name,
    "dovetail",
    new vscode.ShellExecution(`${serverPath} ${args}`),
    ["$dovetail", "$dovetail-test"]
  );
  task.presentationOptions = { reveal: vscode.TaskRevealKind.Always };
  vscode.tasks.executeTask(task);
}

export function activate(context: ExtensionContext) {
  const config = workspace.getConfiguration("dovetail");
  const serverPath = config.get<string>("serverPath", "dovetail");
  const connection = config.get<string>("serverConnection", "stdio");
  const port = config.get<number>("serverPort", 9257);
  const verbose = config.get<boolean>("verbose", false);

  const outputChannel = window.createOutputChannel("Dovetail Language Server");
  const traceOutputChannel = window.createOutputChannel(
    "Dovetail Language Server Trace"
  );

  outputChannel.appendLine(
    `Starting Dovetail Language Server (connection=${connection}, server=${serverPath}, port=${port}, verbose=${verbose})`
  );

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "dovetail" }],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/*.dove"),
    },
    outputChannel,
    traceOutputChannel,
  };

  let serverOptions: ServerOptions;

  if (connection === "tcp") {
    const maxRetries = 30;
    const retryDelayMs = 2000;

    serverOptions = () => {
      return new Promise<StreamInfo>((resolve, reject) => {
        let attempt = 0;

        function tryConnect() {
          attempt++;
          outputChannel.appendLine(
            `Connecting to LSP server on port ${port} (attempt ${attempt}/${maxRetries})...`
          );
          const socket = net.connect({ port }, () => {
            outputChannel.appendLine(`Connected to LSP server on port ${port}`);
            resolve({ reader: socket, writer: socket });
          });
          socket.on("error", (err) => {
            outputChannel.appendLine(`TCP connection error: ${err.message}`);
            if (attempt < maxRetries) {
              outputChannel.appendLine(
                `Retrying in ${retryDelayMs / 1000}s...`
              );
              setTimeout(tryConnect, retryDelayMs);
            } else {
              reject(
                new Error(
                  `Failed to connect after ${maxRetries} attempts: ${err.message}`
                )
              );
            }
          });
        }

        tryConnect();
      });
    };

    // Always reconnect when the TCP connection drops
    clientOptions.errorHandler = {
      error(_error, _message, _count) {
        return { action: ErrorAction.Continue };
      },
      closed() {
        outputChannel.appendLine(
          "Connection to LSP server lost, will try to reconnect..."
        );
        return { action: CloseAction.Restart };
      },
    };
  } else {
    const args = ["lsp-server"];
    if (verbose) {
      args.push("--verbose");
    }
    serverOptions = {
      command: serverPath,
      args,
    };
  }

  client = new LanguageClient(
    "dovetail",
    "Dovetail Language Server",
    serverOptions,
    clientOptions
  );

  // Register test runner commands
  context.subscriptions.push(
    commands.registerCommand("dovetail.runTest", (fqtn: string) => {
      runDovetailTask("Run Test", `test --filter "${fqtn}" --verbose`);
    })
  );

  context.subscriptions.push(
    commands.registerCommand("dovetail.runTestFile", (filePath: string) => {
      runDovetailTask("Run Tests in File", `test --file "${filePath}" --verbose`);
    })
  );

  context.subscriptions.push(
    commands.registerCommand("dovetail.runTestsInProject", async (projectName?: string) => {
      if (!projectName) {
        projectName = await window.showInputBox({
          prompt: "Enter project name",
          placeHolder: "project-name",
        });
        if (!projectName) {
          return;
        }
      }
      runDovetailTask("Run Tests in Project", `test ${projectName} --verbose`);
    })
  );

  context.subscriptions.push(
    commands.registerCommand("dovetail.runTestsInWorkspace", () => {
      runDovetailTask("Run Tests in Workspace", "test --verbose");
    })
  );

  client.start().then(
    () => {
      outputChannel.appendLine("Dovetail Language Server started successfully");
    },
    (err) => {
      outputChannel.appendLine(`Failed to start Dovetail Language Server: ${err}`);
    }
  );
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
