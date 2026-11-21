import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { runTests } from '@vscode/test-electron';

/** Launches VS Code with this extension and the stand-in server, then runs `suite/index`. */
async function main(): Promise<void> {
  const root = path.resolve(__dirname, '..', '..');
  const fixtures = path.join(root, 'test-fixtures');
  const server = path.join(fixtures, 'standin-server.js');
  fs.chmodSync(server, 0o755);

  // A throw-away profile whose settings point the extension at the stand-in server.
  const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'lp-vscode-test-'));
  fs.mkdirSync(path.join(userData, 'User'), { recursive: true });
  fs.writeFileSync(path.join(userData, 'User', 'settings.json'), JSON.stringify({ 'lp.server.path': server, 'workbench.startupEditor': 'none' }));

  try {
    await runTests({
      extensionDevelopmentPath: root,
      extensionTestsPath: path.join(__dirname, 'suite', 'index'),
      launchArgs: [path.join(fixtures, 'workspace'), '--user-data-dir', userData, '--disable-extensions', '--disable-workspace-trust'],
    });
  } finally {
    fs.rmSync(userData, { recursive: true, force: true });
  }
}

main().catch((error: unknown) => {
  console.error('Extension tests failed:', error);
  process.exit(1);
});
