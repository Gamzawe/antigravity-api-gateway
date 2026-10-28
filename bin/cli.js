#!/usr/bin/env node

import { fileURLToPath } from 'url';
import { dirname, join } from 'path';
import { readFileSync } from 'fs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const packageJson = JSON.parse(
  readFileSync(join(__dirname, '..', 'package.json'), 'utf-8')
);

const args = process.argv.slice(2);
const command = args[0];

function showHelp() {
  console.log(`
antigravity-gateway v${packageJson.version}

Universal AI gateway - access Claude & Gemini via OpenAI or Anthropic-compatible API.

USAGE:
  antigravity-gateway <command> [options]
  agw <command> [options]

COMMANDS:
  start                 Start the gateway server (default port: 8080)
  accounts              Manage Google accounts (interactive)
  accounts add          Add a new Google account via OAuth
  accounts list         List all configured accounts
  accounts remove       Remove accounts interactively
  accounts verify       Verify account tokens are valid
  accounts clear        Remove all accounts

OPTIONS:
  --help, -h            Show this help message
  --version, -v         Show version number
  --debug               Enable debug logging
  --fallback            Enable model fallback on quota exhaustion

ENVIRONMENT:
  PORT                  Server port (default: 8080)
  DEBUG                 Enable debug mode (true/false)
  FALLBACK              Enable model fallback (true/false)

EXAMPLES:
  antigravity-gateway start
  agw start --debug
  PORT=3000 agw start
  agw accounts add
  agw accounts add --no-browser

SUPPORTED CLIENTS:
  Works with any OpenAI or Anthropic-compatible client:
  - Cursor, Continue, Cline, Roo Code, Kilo Code
  - Aider, Cody, Claude Code, Gemini CLI
  - OpenAI Python/JS SDK, LiteLLM
  - Cherry Studio, Goose, and more...

QUICK START:
  Base URL: http://localhost:8080/v1
  API Key:  any-value (not validated)
`);
}

function showVersion() {
  console.log(packageJson.version);
}

async function main() {
  if (args.includes('--help') || args.includes('-h')) {
    showHelp();
    process.exit(0);
  }

  if (args.includes('--version') || args.includes('-v')) {
    showVersion();
    process.exit(0);
  }

  switch (command) {
    case 'start':
    case undefined:
      await import('../src/index.js');
      break;

    case 'accounts': {
      const subCommand = args[1] || 'add';
      process.argv = ['node', 'accounts-cli.js', subCommand, ...args.slice(2)];
      await import('../src/cli/accounts.js');
      break;
    }

    case 'help':
      showHelp();
      break;

    case 'version':
      showVersion();
      break;

    default:
      console.error(`Unknown command: ${command}`);
      console.error('Run "agw --help" for usage information.');
      process.exit(1);
  }
}

main().catch((err) => {
  console.error('Error:', err.message);
  process.exit(1);
});
