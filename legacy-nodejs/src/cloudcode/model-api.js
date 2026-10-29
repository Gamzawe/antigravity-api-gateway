/**
 * Model API for Cloud Code
 *
 * Handles model listing and quota retrieval from the Cloud Code API.
 */

import { ANTIGRAVITY_ENDPOINT_FALLBACKS, ANTIGRAVITY_HEADERS, getModelFamily, MODEL_ALIASES } from '../constants.js';
import { logger } from '../utils/logger.js';

/**
 * Check if a model is supported (Claude or Gemini)
 * @param {string} modelId - Model ID to check
 * @returns {boolean} True if model is supported
 */
function isSupportedModel(modelId) {
    const family = getModelFamily(modelId);
    return family === 'claude' || family === 'gemini';
}

/**
 * List available models in Anthropic & OpenAI API format
 * Fetches models dynamically from the Cloud Code API + common aliases
 *
 * @param {string} token - OAuth access token
 * @param {string} [project] - Project ID
 * @returns {Promise<{object: string, data: Array<{id: string, object: string, created: number, owned_by: string, description: string}>}>} List of available models
 */
export async function listModels(token, project = 'aicode-consumers') {
    const data = await fetchAvailableModels(token, project);
    if (!data || !data.models) {
        return { object: 'list', data: [] };
    }

    const seen = new Set();
    const modelList = [];

    // Add models returned directly from CloudCode
    for (const [modelId, modelData] of Object.entries(data.models)) {
        if (isSupportedModel(modelId)) {
            seen.add(modelId);
            modelList.push({
                id: modelId,
                object: 'model',
                created: Math.floor(Date.now() / 1000),
                owned_by: modelId.includes('claude') ? 'anthropic' : 'google',
                description: modelData.displayName || modelId
            });
        }
    }

    // Also include clean convenience aliases (e.g. gemini-3.8-flash, gemini-pro, claude-sonnet)
    for (const [alias, target] of Object.entries(MODEL_ALIASES)) {
        if (!seen.has(alias)) {
            seen.add(alias);
            modelList.push({
                id: alias,
                object: 'model',
                created: Math.floor(Date.now() / 1000),
                owned_by: alias.includes('claude') ? 'anthropic' : 'google',
                description: `Alias -> ${target}`
            });
        }
    }

    return {
        object: 'list',
        data: modelList
    };
}

/**
 * Fetch available models with quota info from Cloud Code API
 * Returns model quotas including remaining fraction and reset time
 *
 * @param {string} token - OAuth access token
 * @param {string} [project] - Project ID
 * @returns {Promise<Object>} Raw response from fetchAvailableModels API
 */
export async function fetchAvailableModels(token, project = 'aicode-consumers') {
    const headers = {
        'Authorization': `Bearer ${token}`,
        'Content-Type': 'application/json',
        ...ANTIGRAVITY_HEADERS
    };

    for (const endpoint of ANTIGRAVITY_ENDPOINT_FALLBACKS) {
        try {
            const url = `${endpoint}/v1internal:fetchAvailableModels`;
            const response = await fetch(url, {
                method: 'POST',
                headers,
                body: JSON.stringify({ project: project || 'aicode-consumers' })
            });

            if (!response.ok) {
                const errorText = await response.text();
                logger.warn(`[CloudCode] fetchAvailableModels error at ${endpoint}: ${response.status}`);
                continue;
            }

            return await response.json();
        } catch (error) {
            logger.warn(`[CloudCode] fetchAvailableModels failed at ${endpoint}:`, error.message);
        }
    }

    throw new Error('Failed to fetch available models from all endpoints');
}

/**
 * Get model quotas for an account
 * Extracts quota info (remaining fraction and reset time) for each model
 *
 * @param {string} token - OAuth access token
 * @returns {Promise<Object>} Map of modelId -> { remainingFraction, resetTime }
 */
export async function getModelQuotas(token) {
    const data = await fetchAvailableModels(token);
    if (!data || !data.models) return {};

    const quotas = {};
    for (const [modelId, modelData] of Object.entries(data.models)) {
        // Only include Claude and Gemini models
        if (!isSupportedModel(modelId)) continue;

        if (modelData.quotaInfo) {
            quotas[modelId] = {
                remainingFraction: modelData.quotaInfo.remainingFraction ?? null,
                resetTime: modelData.quotaInfo.resetTime ?? null
            };
        }
    }

    return quotas;
}
