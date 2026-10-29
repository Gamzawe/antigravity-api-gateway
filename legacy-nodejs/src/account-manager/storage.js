/**
 * Account Storage
 *
 * Handles loading and saving account configuration to disk.
 */

import { readFile, writeFile, mkdir, access, chmod, rename, unlink } from 'fs/promises';
import { randomBytes } from 'node:crypto';
import { constants as fsConstants, existsSync } from 'fs';
import { dirname, join } from 'path';
import { homedir } from 'os';
import { ACCOUNT_CONFIG_PATH, DATA_DIR } from '../constants.js';
import { getAuthStatus } from '../auth/database.js';
import { logger } from '../utils/logger.js';

/**
 * Find valid accounts file path
 */
function resolveAccountsPath(preferredPath) {
    const candidatePaths = [
        preferredPath,
        ACCOUNT_CONFIG_PATH,
        join(DATA_DIR, 'accounts.json'),
        join(process.cwd(), 'data', 'accounts.json'),
        join(homedir(), '.config', 'antigravity-gateway', 'accounts.json'),
        join(homedir(), '.config', 'opencode', 'antigravity-accounts.json')
    ].filter(Boolean);

    for (const p of candidatePaths) {
        if (existsSync(p)) return p;
    }
    return preferredPath || ACCOUNT_CONFIG_PATH;
}

/**
 * Load accounts from the config file
 *
 * @param {string} configPath - Path to the config file
 * @returns {Promise<{accounts: Array, settings: Object, activeIndex: number, loadedPath: string}>}
 */
export async function loadAccounts(configPath = ACCOUNT_CONFIG_PATH) {
    const resolvedPath = process.env.ACCOUNTS_FILE ? configPath : resolveAccountsPath(configPath);
    try {
        await access(resolvedPath, fsConstants.F_OK);
        const configData = await readFile(resolvedPath, 'utf-8');
        const config = JSON.parse(configData);

        const rawList = Array.isArray(config.accounts) ? config.accounts : (Array.isArray(config) ? config : []);

        const accounts = rawList.map(acc => ({
            ...acc,
            source: acc.source || (acc.refreshToken ? 'oauth' : (acc.apiKey ? 'manual' : 'database')),
            projectId: acc.managedProjectId || acc.projectId || 'aicode-consumers',
            lastUsed: acc.lastUsed || null,
            // Reset invalid flag on startup - give accounts a fresh chance to refresh
            isInvalid: false,
            invalidReason: null,
            modelRateLimits: acc.modelRateLimits || {}
        }));

        const settings = config.settings || {};
        let activeIndex = config.activeIndex || 0;

        // Clamp activeIndex to valid range
        if (activeIndex >= accounts.length) {
            activeIndex = 0;
        }

        logger.info(`[AccountManager] Loaded ${accounts.length} account(s) from ${resolvedPath}`);

        return { accounts, settings, activeIndex, loadedPath: resolvedPath };
    } catch (error) {
        if (error.code === 'ENOENT') {
            logger.info(`[AccountManager] No accounts file found at ${resolvedPath}.`);
        } else {
            logger.error('[AccountManager] Failed to load config:', error.message);
        }
        return { accounts: [], settings: {}, activeIndex: 0, loadedPath: resolvedPath };
    }
}

/**
 * Load the default account from Antigravity's database
 *
 * @param {string} dbPath - Optional path to the database
 * @returns {{accounts: Array, tokenCache: Map}}
 */
export function loadDefaultAccount(dbPath) {
    try {
        const authData = getAuthStatus(dbPath);
        if (authData?.apiKey) {
            const account = {
                email: authData.email || 'default@antigravity',
                source: 'database',
                lastUsed: null,
                modelRateLimits: {}
            };

            const tokenCache = new Map();
            tokenCache.set(account.email, {
                token: authData.apiKey,
                extractedAt: Date.now()
            });

            logger.info(`[AccountManager] Loaded default account: ${account.email}`);

            return { accounts: [account], tokenCache };
        }
    } catch (error) {
        logger.error('[AccountManager] Failed to load default account:', error.message);
    }

    return { accounts: [], tokenCache: new Map() };
}

let saveQueue = Promise.resolve();

/**
 * Save accounts to config file atomically and sequentially
 *
 * @param {string} configPath - Path to config file
 * @param {Array} accounts - Array of account objects
 * @param {Object} settings - Settings object
 * @param {number} activeIndex - Current active account index
 */
export async function saveAccounts(configPath, accounts, settings, activeIndex) {
    // Chain saves sequentially to prevent concurrent write collisions
    saveQueue = saveQueue.then(async () => {
        try {
            // Ensure directory exists
            const dir = dirname(configPath);
            await mkdir(dir, { recursive: true, mode: 0o700 });
            if (process.platform !== 'win32') await chmod(dir, 0o700);

            const config = {
                accounts: accounts.map(acc => ({
                    email: acc.email,
                    source: acc.source || 'oauth',
                    dbPath: acc.dbPath || null,
                    refreshToken: acc.refreshToken,
                    apiKey: acc.source === 'manual' ? acc.apiKey : undefined,
                    projectId: acc.projectId || 'aicode-consumers',
                    managedProjectId: acc.managedProjectId || acc.projectId || 'aicode-consumers',
                    enabled: acc.enabled !== false,
                    addedAt: acc.addedAt || undefined,
                    isInvalid: acc.isInvalid || false,
                    invalidReason: acc.invalidReason || null,
                    modelRateLimits: acc.modelRateLimits || {},
                    lastUsed: acc.lastUsed
                })),
                settings: settings,
                activeIndex: activeIndex
            };

            // Never open an existing credential file with inherited loose permissions.
            // A private temporary file also avoids partial JSON on interruption.
            const temporary = configPath + '.' + randomBytes(8).toString('hex') + '.tmp';
            try {
                await writeFile(temporary, JSON.stringify(config, null, 2), { mode: 0o600, flag: 'wx', flush: true });
                await rename(temporary, configPath);
                if (process.platform !== 'win32') await chmod(configPath, 0o600);
            } finally {
                await unlink(temporary).catch(error => { if (error.code !== 'ENOENT') throw error; });
            }
        } catch (error) {
            logger.error('[AccountManager] Failed to save config:', error.message);
        }
    });

    return await saveQueue;
}
